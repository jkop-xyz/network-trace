//! Sending a single ICMP echo Probe with a given TTL, without admin rights.
//!
//! Linux uses unprivileged ICMP datagram sockets (`net.ipv4.ping_group_range`),
//! Windows uses the IP Helper ICMP API. Both are blocking; callers run them on
//! a blocking thread pool.

use std::io;
use std::net::IpAddr;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// What came back for one Probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The Target itself answered the echo request.
    Echo { from: IpAddr, rtt: Duration },
    /// A router on the way reported that the TTL ran out.
    TimeExceeded { from: IpAddr, rtt: Duration },
    /// A router (or the Target) reported the destination as unreachable.
    Unreachable { from: IpAddr, rtt: Duration },
    /// Nothing came back before the timeout.
    Timeout,
}

impl ProbeOutcome {
    pub fn responder(&self) -> Option<(IpAddr, Duration)> {
        match *self {
            ProbeOutcome::Echo { from, rtt }
            | ProbeOutcome::TimeExceeded { from, rtt }
            | ProbeOutcome::Unreachable { from, rtt } => Some((from, rtt)),
            ProbeOutcome::Timeout => None,
        }
    }
}

/// Payload carried in every echo request, so captures are recognisable.
pub(crate) const PAYLOAD: &[u8; 32] = b"network-trace probe payload 0123";

/// Send one echo request to `target` with the given TTL and wait up to
/// `timeout` for any answer.
pub fn probe(target: IpAddr, ttl: u8, timeout: Duration) -> io::Result<ProbeOutcome> {
    #[cfg(target_os = "linux")]
    {
        linux::probe(target, ttl, timeout)
    }
    #[cfg(windows)]
    {
        windows::probe(target, ttl, timeout)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (target, ttl, timeout);
        Err(io::Error::new(io::ErrorKind::Unsupported, "only Linux and Windows are supported"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_answers_with_echo() {
        let out = probe("127.0.0.1".parse().unwrap(), 64, Duration::from_secs(1)).unwrap();
        match out {
            ProbeOutcome::Echo { from, .. } => assert_eq!(from, "127.0.0.1".parse::<IpAddr>().unwrap()),
            other => panic!("expected echo, got {other:?}"),
        }
    }

    #[test]
    fn loopback_v6_answers_with_echo() {
        let Ok(out) = probe("::1".parse().unwrap(), 64, Duration::from_secs(1)) else {
            return; // IPv6 disabled on this machine
        };
        assert!(matches!(out, ProbeOutcome::Echo { .. }), "got {out:?}");
    }

    #[test]
    fn unroutable_address_times_out() {
        // TEST-NET-1 (RFC 5737) is never routed.
        let out = probe("192.0.2.1".parse().unwrap(), 64, Duration::from_millis(300));
        match out {
            Ok(ProbeOutcome::Timeout) | Ok(ProbeOutcome::Unreachable { .. }) | Err(_) => {}
            Ok(other) => panic!("unexpected {other:?}"),
        }
    }
}
