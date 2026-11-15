//! Linux: unprivileged `SOCK_DGRAM` ICMP sockets.
//!
//! The kernel rewrites the echo identifier per socket and delivers only the
//! matching echo replies. ICMP errors (TTL exceeded, unreachable) for our
//! packets arrive on the socket's error queue once `IP_RECVERR` is enabled.
//! One socket per Probe keeps matching trivial.

use super::{PAYLOAD, ProbeOutcome};
use std::io;
use std::mem::{size_of, zeroed};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

const ICMP_ECHO_REQUEST: u8 = 8;
const ICMP_ECHO_REPLY: u8 = 0;
const ICMP_DEST_UNREACH: u8 = 3;
const ICMP_TIME_EXCEEDED: u8 = 11;
const ICMP6_ECHO_REQUEST: u8 = 128;
const ICMP6_ECHO_REPLY: u8 = 129;
const ICMP6_DEST_UNREACH: u8 = 1;
const ICMP6_TIME_EXCEEDED: u8 = 3;
const SEQ: u16 = 1;

pub fn probe(target: IpAddr, ttl: u8, timeout: Duration) -> io::Result<ProbeOutcome> {
    let v6 = target.is_ipv6();
    let sock = open_socket(v6, ttl)?;
    let fd = sock.as_raw_fd();

    let packet = echo_request(v6);
    let start = Instant::now();
    send_to(fd, &packet, target)?;
    let deadline = start + timeout;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(ProbeOutcome::Timeout);
        }
        let wait_ms = (deadline - now).as_millis().clamp(1, i32::MAX as u128) as i32;
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN | libc::POLLERR, revents: 0 };
        let n = unsafe { libc::poll(&mut pfd, 1, wait_ms) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if n == 0 {
            return Ok(ProbeOutcome::Timeout);
        }
        if pfd.revents & libc::POLLERR != 0 {
            if let Some(out) = read_error_queue(fd, v6, start)? {
                return Ok(out);
            }
        }
        if pfd.revents & libc::POLLIN != 0 {
            if let Some(out) = read_reply(fd, v6, start)? {
                return Ok(out);
            }
        }
    }
}

fn open_socket(v6: bool, ttl: u8) -> io::Result<OwnedFd> {
    let (domain, proto) =
        if v6 { (libc::AF_INET6, libc::IPPROTO_ICMPV6) } else { (libc::AF_INET, libc::IPPROTO_ICMP) };
    let fd = unsafe { libc::socket(domain, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, proto) };
    if fd < 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EACCES) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unprivileged ICMP is disabled; allow it with `sysctl net.ipv4.ping_group_range=\"0 2147483647\"`",
            ));
        }
        return Err(err);
    }
    let sock = unsafe { OwnedFd::from_raw_fd(fd) };
    let ttl = ttl as libc::c_int;
    if v6 {
        set_int(fd, libc::IPPROTO_IPV6, libc::IPV6_UNICAST_HOPS, ttl)?;
        set_int(fd, libc::IPPROTO_IPV6, libc::IPV6_RECVERR, 1)?;
    } else {
        set_int(fd, libc::IPPROTO_IP, libc::IP_TTL, ttl)?;
        set_int(fd, libc::IPPROTO_IP, libc::IP_RECVERR, 1)?;
    }
    Ok(sock)
}

fn set_int(fd: i32, level: i32, name: i32, value: libc::c_int) -> io::Result<()> {
    let rc = unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            &value as *const _ as *const libc::c_void,
            size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

fn echo_request(v6: bool) -> Vec<u8> {
    let mut p = Vec::with_capacity(8 + PAYLOAD.len());
    p.push(if v6 { ICMP6_ECHO_REQUEST } else { ICMP_ECHO_REQUEST });
    p.push(0); // code
    p.extend_from_slice(&[0, 0]); // checksum, filled below (kernel does it for v6)
    p.extend_from_slice(&[0, 0]); // identifier, overwritten by the kernel
    p.extend_from_slice(&SEQ.to_be_bytes());
    p.extend_from_slice(PAYLOAD);
    if !v6 {
        let sum = checksum(&p);
        p[2..4].copy_from_slice(&sum.to_be_bytes());
    }
    p
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 { u16::from_be_bytes([chunk[0], chunk[1]]) } else { u16::from(chunk[0]) << 8 };
        sum += u32::from(word);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn send_to(fd: i32, packet: &[u8], target: IpAddr) -> io::Result<()> {
    let (storage, len) = to_sockaddr(target);
    let rc = unsafe {
        libc::sendto(
            fd,
            packet.as_ptr() as *const libc::c_void,
            packet.len(),
            0,
            &storage as *const _ as *const libc::sockaddr,
            len,
        )
    };
    if rc < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

fn to_sockaddr(ip: IpAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    let mut storage: libc::sockaddr_storage = unsafe { zeroed() };
    match ip {
        IpAddr::V4(v4) => {
            let sin = &mut storage as *mut _ as *mut libc::sockaddr_in;
            unsafe {
                (*sin).sin_family = libc::AF_INET as libc::sa_family_t;
                (*sin).sin_addr.s_addr = u32::from_ne_bytes(v4.octets());
            }
            (storage, size_of::<libc::sockaddr_in>() as libc::socklen_t)
        }
        IpAddr::V6(v6) => {
            let sin6 = &mut storage as *mut _ as *mut libc::sockaddr_in6;
            unsafe {
                (*sin6).sin6_family = libc::AF_INET6 as libc::sa_family_t;
                (*sin6).sin6_addr.s6_addr = v6.octets();
            }
            (storage, size_of::<libc::sockaddr_in6>() as libc::socklen_t)
        }
    }
}

unsafe fn from_sockaddr(sa: *const libc::sockaddr) -> Option<IpAddr> {
    unsafe {
        match (*sa).sa_family as i32 {
            libc::AF_INET => {
                let sin = sa as *const libc::sockaddr_in;
                Some(IpAddr::V4(Ipv4Addr::from((*sin).sin_addr.s_addr.to_ne_bytes())))
            }
            libc::AF_INET6 => {
                let sin6 = sa as *const libc::sockaddr_in6;
                Some(IpAddr::V6(Ipv6Addr::from((*sin6).sin6_addr.s6_addr)))
            }
            _ => None,
        }
    }
}

/// Reads one echo reply. Returns `None` for packets that aren't ours.
fn read_reply(fd: i32, v6: bool, start: Instant) -> io::Result<Option<ProbeOutcome>> {
    let mut buf = [0u8; 1500];
    let mut from: libc::sockaddr_storage = unsafe { zeroed() };
    let mut from_len = size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let n = unsafe {
        libc::recvfrom(
            fd,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
            libc::MSG_DONTWAIT,
            &mut from as *mut _ as *mut libc::sockaddr,
            &mut from_len,
        )
    };
    if n < 0 {
        let err = io::Error::last_os_error();
        return if err.kind() == io::ErrorKind::WouldBlock { Ok(None) } else { Err(err) };
    }
    let rtt = start.elapsed();
    let n = n as usize;
    let reply_type = if v6 { ICMP6_ECHO_REPLY } else { ICMP_ECHO_REPLY };
    if n < 8 || buf[0] != reply_type || u16::from_be_bytes([buf[6], buf[7]]) != SEQ {
        return Ok(None);
    }
    let Some(from) = (unsafe { from_sockaddr(&from as *const _ as *const libc::sockaddr) }) else {
        return Ok(None);
    };
    Ok(Some(ProbeOutcome::Echo { from, rtt }))
}

/// Reads one ICMP error for our packet from the socket's error queue.
fn read_error_queue(fd: i32, v6: bool, start: Instant) -> io::Result<Option<ProbeOutcome>> {
    let mut data = [0u8; 1500];
    let mut control = [0u64; 64]; // u64 for cmsg alignment
    let mut name: libc::sockaddr_storage = unsafe { zeroed() };
    let mut iov = libc::iovec { iov_base: data.as_mut_ptr() as *mut libc::c_void, iov_len: data.len() };
    let mut msg: libc::msghdr = unsafe { zeroed() };
    msg.msg_name = &mut name as *mut _ as *mut libc::c_void;
    msg.msg_namelen = size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = size_of::<[u64; 64]>() as _;

    let n = unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_ERRQUEUE | libc::MSG_DONTWAIT) };
    if n < 0 {
        let err = io::Error::last_os_error();
        return if err.kind() == io::ErrorKind::WouldBlock { Ok(None) } else { Err(err) };
    }
    let rtt = start.elapsed();
    let n = n as usize;
    // The queued data is the ICMP packet we sent; make sure it's ours.
    if n >= 8 && u16::from_be_bytes([data[6], data[7]]) != SEQ {
        return Ok(None);
    }

    let (level, kind, origin_icmp) = if v6 {
        (libc::IPPROTO_IPV6, libc::IPV6_RECVERR, libc::SO_EE_ORIGIN_ICMP6)
    } else {
        (libc::IPPROTO_IP, libc::IP_RECVERR, libc::SO_EE_ORIGIN_ICMP)
    };

    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    while !cmsg.is_null() {
        let hdr = unsafe { &*cmsg };
        if hdr.cmsg_level == level && hdr.cmsg_type == kind {
            let ee = unsafe { libc::CMSG_DATA(cmsg) } as *const libc::sock_extended_err;
            let ee_ref = unsafe { &*ee };
            if ee_ref.ee_origin != origin_icmp {
                // A local error (e.g. no route): the Probe is lost.
                return Ok(Some(ProbeOutcome::Timeout));
            }
            // SO_EE_OFFENDER: the address that sent the ICMP error follows the struct.
            let offender = unsafe { ee.add(1) } as *const libc::sockaddr;
            let Some(from) = (unsafe { from_sockaddr(offender) }) else { return Ok(None) };
            let (time_exceeded, unreach) =
                if v6 { (ICMP6_TIME_EXCEEDED, ICMP6_DEST_UNREACH) } else { (ICMP_TIME_EXCEEDED, ICMP_DEST_UNREACH) };
            return Ok(Some(match ee_ref.ee_type {
                t if t == time_exceeded => ProbeOutcome::TimeExceeded { from, rtt },
                t if t == unreach => ProbeOutcome::Unreachable { from, rtt },
                _ => return Ok(None),
            }));
        }
        cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_of_known_packet() {
        // Echo request, id 0, seq 1, no payload: checksum is !0x0801 = 0xf7fe.
        assert_eq!(checksum(&[8, 0, 0, 0, 0, 0, 0, 1]), 0xf7fe);
    }
}
