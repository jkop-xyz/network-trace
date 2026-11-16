//! Running a Session: send a Sample every interval, turn the Probe outcomes
//! into recorded Hops, and append them to the Session file.

use crate::model::{HopResult, Millis, Sample, now_ms};
use crate::probe::{ProbeOutcome, probe};
use crate::store::SessionFile;
use std::io;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep_until};

/// Routes longer than this are not followed.
pub const MAX_HOPS: u8 = 30;
/// While the route is unknown, probe this many TTLs past the last router
/// that answered (like mtr), instead of all of them.
const PROBE_PAST_LAST_RESPONDER: u8 = 4;
/// While the route is unknown, every this many Samples probe all TTLs again
/// in case the route has grown past the routers seen so far.
const FULL_PROBE_EVERY: u32 = 30;

pub type Prober = dyn Fn(IpAddr, u8, Duration) -> io::Result<ProbeOutcome> + Send + Sync;

/// The real prober: ICMP on the current platform.
pub fn icmp_prober() -> Arc<Prober> {
    Arc::new(probe)
}

/// What is known about the route to the Target between Samples.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RouteState {
    /// Hop number at which the Target answers, once it has.
    pub length: Option<u8>,
    /// Highest TTL that got any answer, while `length` is unknown.
    pub last_responder: Option<u8>,
    /// Samples sent since the last one that probed every TTL.
    pub since_full_probe: u32,
    /// The router last seen answering at each Hop, to recognise it when it
    /// answers "destination unreachable" for its own Hop.
    pub hop_addresses: [Option<IpAddr>; MAX_HOPS as usize],
}

impl RouteState {
    /// How many TTLs the next Sample should probe.
    pub fn ttls_to_probe(&mut self) -> u8 {
        if let Some(len) = self.length {
            return len;
        }
        match self.last_responder {
            Some(last) if self.since_full_probe < FULL_PROBE_EVERY => {
                self.since_full_probe += 1;
                last.saturating_add(PROBE_PAST_LAST_RESPONDER).min(MAX_HOPS)
            }
            _ => {
                self.since_full_probe = 0;
                MAX_HOPS
            }
        }
    }
}

/// The Hops to record for one Sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    pub hops: Vec<HopResult>,
    /// True when the last recorded Hop is the Target's position, whether or
    /// not it answered this time.
    pub target_known: bool,
}

/// Turns one Sample's Probe outcomes (indexed by TTL, starting at 1) into the
/// Hops to record, and updates what is known about the route.
///
/// - Target answered at TTL `n`: record Hops 1..=n; the route is `n` long.
/// - Target silent but the route length is known: record 1..=known, so a dead
///   connection shows as loss at every Hop including the final one. If a router
///   now answers "TTL exceeded" at the known final position, the route has grown
///   and is re-discovered next Sample.
/// - Nothing known yet: record up to the last Hop that answered.
///
/// A "destination unreachable" from a router means the packet didn't get
/// through, e.g. the home router's internet link is down: the Hops it answers
/// for count as lost, with the router noted in `unreachable_from`. The one
/// exception is the router's own Hop: a router with no route onward answers
/// even TTL-limited Probes that way, and it did answer, so that Hop counts as
/// answered. Otherwise a dead internet link would look like a fault in the
/// local network.
pub fn assemble(target: IpAddr, outcomes: &[io::Result<ProbeOutcome>], state: &mut RouteState) -> Assembled {
    let reached = outcomes.iter().position(|o| match o {
        Ok(ProbeOutcome::Echo { .. }) => true,
        Ok(ProbeOutcome::Unreachable { from, .. }) => *from == target,
        _ => false,
    });
    let known_at = state.hop_addresses;
    // Whether the Probe sent with TTL `i + 1` got an answer for that Hop.
    let answered_at = |i: usize, o: &io::Result<ProbeOutcome>| match o {
        Ok(ProbeOutcome::Unreachable { from, .. }) => *from == target || known_at.get(i) == Some(&Some(*from)),
        Ok(o) => o.responder().is_some(),
        Err(_) => false,
    };

    let (count, target_known) = match (reached, state.length) {
        (Some(i), _) => {
            state.length = Some(i as u8 + 1);
            (i + 1, true)
        }
        (None, Some(known)) => {
            let known = (known as usize).min(outcomes.len());
            if let Some(Ok(ProbeOutcome::TimeExceeded { .. })) = outcomes.get(known.wrapping_sub(1)) {
                // A router now sits where the Target was: the route grew.
                state.length = None;
                state.last_responder = Some(known as u8);
                (known, false)
            } else {
                (known, true)
            }
        }
        (None, None) => {
            let last = outcomes.iter().enumerate().rposition(|(i, o)| answered_at(i, o));
            if let Some(i) = last {
                state.last_responder = Some(i as u8 + 1);
            }
            (last.map(|i| i + 1).unwrap_or(1).min(outcomes.len()), false)
        }
    };

    let hops = outcomes[..count]
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let responder = if answered_at(i, o) { o.as_ref().ok().and_then(|o| o.responder()) } else { None };
            let unreachable_from = match o {
                Ok(ProbeOutcome::Unreachable { from, .. }) if responder.is_none() => Some(*from),
                _ => None,
            };
            HopResult {
                hop: i as u8 + 1,
                address: responder.map(|(a, _)| a),
                rtt_us: responder.map(|(_, rtt)| rtt.as_micros().min(u32::MAX as u128) as u32),
                unreachable_from,
            }
        })
        .collect::<Vec<_>>();
    for h in &hops {
        if let (Some(a), Some(slot)) = (h.address, state.hop_addresses.get_mut(h.hop as usize - 1)) {
            *slot = Some(a);
        }
    }
    Assembled { hops, target_known }
}

/// Controls for a running Session.
pub struct SessionControl {
    interval: watch::Sender<u32>,
    stop: watch::Sender<bool>,
}

impl SessionControl {
    pub fn set_interval(&self, interval_ms: u32) {
        self.interval.send_replace(interval_ms);
    }
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }
}

pub struct SessionConfig {
    pub target: IpAddr,
    pub interval_ms: u32,
    pub timeout: Duration,
}

/// Starts sending Samples on the current Tokio runtime. `on_sample` runs after
/// each Sample is stored. The returned future resolves once the Session has
/// stopped and its stop time is recorded; it is an error if storing Samples
/// failed, in which case the Session stops early.
pub fn start(
    file: SessionFile,
    config: SessionConfig,
    prober: Arc<Prober>,
    on_sample: impl Fn(&Sample) + Send + 'static,
) -> (SessionControl, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let (interval_tx, interval_rx) = watch::channel(config.interval_ms);
    let (stop_tx, stop_rx) = watch::channel(false);
    let handle = tokio::spawn(run(file, config, prober, interval_rx, stop_rx, on_sample));
    (SessionControl { interval: interval_tx, stop: stop_tx }, handle)
}

type RawSample = (Millis, u32, Vec<io::Result<ProbeOutcome>>);

async fn run(
    file: SessionFile,
    config: SessionConfig,
    prober: Arc<Prober>,
    mut interval_rx: watch::Receiver<u32>,
    mut stop_rx: watch::Receiver<bool>,
    on_sample: impl Fn(&Sample) + Send + 'static,
) -> anyhow::Result<()> {
    let target = config.target;
    let route = Arc::new(std::sync::Mutex::new(RouteState::default()));
    let (tx, mut rx) = mpsc::unbounded_channel::<RawSample>();

    // A single writer owns the Session file and the route state. It hands
    // the file back with the first write error, if any.
    let writer_route = route.clone();
    let writer = tokio::task::spawn_blocking(move || -> (SessionFile, anyhow::Result<()>) {
        let mut file = file;
        while let Some((ts, interval_ms, outcomes)) = rx.blocking_recv() {
            let Assembled { hops, target_known } = assemble(target, &outcomes, &mut writer_route.lock().unwrap());
            let sample = Sample { ts, interval_ms, target_known, hops };
            if let Err(e) = file.append(&sample) {
                return (file, Err(e.context("could not save Samples")));
            }
            on_sample(&sample);
        }
        (file, Ok(()))
    });

    // Samples overlap when the timeout is longer than the interval; this caps
    // how many can be in flight so a stalled network can't pile up threads.
    let max_in_flight = (config.timeout.as_millis() as u64 / config.interval_ms.max(1) as u64 + 2) as usize;
    let mut in_flight = JoinSet::new();
    let mut next = Instant::now();
    loop {
        if *stop_rx.borrow() || tx.is_closed() {
            break;
        }
        while in_flight.try_join_next().is_some() {}
        if in_flight.len() < max_in_flight.max(2) {
            let interval_ms = *interval_rx.borrow();
            let ttl_max = route.lock().unwrap().ttls_to_probe();
            let (prober, tx, timeout) = (prober.clone(), tx.clone(), config.timeout);
            let ts = now_ms();
            in_flight.spawn(async move {
                let mut probes = JoinSet::new();
                for ttl in 1..=ttl_max {
                    let prober = prober.clone();
                    probes.spawn_blocking(move || (ttl, prober(target, ttl, timeout)));
                }
                let mut outcomes: Vec<io::Result<ProbeOutcome>> = (0..ttl_max).map(|_| Ok(ProbeOutcome::Timeout)).collect();
                while let Some(res) = probes.join_next().await {
                    if let Ok((ttl, out)) = res {
                        outcomes[ttl as usize - 1] = out;
                    }
                }
                let _ = tx.send((ts, interval_ms, outcomes));
            });
        }

        next += Duration::from_millis(*interval_rx.borrow() as u64);
        let now = Instant::now();
        if next < now {
            // We fell far behind (e.g. the machine slept): restart the cadence.
            next = now;
        }
        tokio::select! {
            _ = sleep_until(next) => {}
            changed = stop_rx.changed() => if changed.is_err() { break },
            changed = interval_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                // Apply a new interval from now on.
                next = Instant::now() + Duration::from_millis(*interval_rx.borrow() as u64);
                tokio::select! {
                    _ = sleep_until(next) => {}
                    _ = stop_rx.changed() => {}
                }
            }
        }
    }

    while in_flight.join_next().await.is_some() {}
    drop(tx);
    let (file, result) = writer.await?;
    // Record the stop time even when saving Samples failed.
    let stopped = file.stop_at(now_ms());
    result.and(stopped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }
    fn hop(s: &str, ms: u64) -> io::Result<ProbeOutcome> {
        Ok(ProbeOutcome::TimeExceeded { from: ip(s), rtt: Duration::from_millis(ms) })
    }
    fn echo(s: &str, ms: u64) -> io::Result<ProbeOutcome> {
        Ok(ProbeOutcome::Echo { from: ip(s), rtt: Duration::from_millis(ms) })
    }
    fn unreachable(s: &str) -> io::Result<ProbeOutcome> {
        Ok(ProbeOutcome::Unreachable { from: ip(s), rtt: Duration::from_millis(1) })
    }
    fn lost() -> io::Result<ProbeOutcome> {
        Ok(ProbeOutcome::Timeout)
    }
    fn known(n: u8) -> RouteState {
        RouteState { length: Some(n), ..Default::default() }
    }

    const T: &str = "1.1.1.1";

    #[test]
    fn target_reached_records_up_to_target_and_learns_length() {
        let mut st = RouteState::default();
        let a = assemble(ip(T), &[hop("10.0.0.1", 1), lost(), echo(T, 9), echo(T, 9), echo(T, 9)], &mut st);
        assert_eq!(a.hops.len(), 3);
        assert!(a.target_known);
        assert_eq!(a.hops[1], HopResult { hop: 2, ..Default::default() });
        assert_eq!(a.hops[2], HopResult { hop: 3, address: Some(ip(T)), rtt_us: Some(9000), unreachable_from: None });
        assert_eq!(st.length, Some(3));
    }

    #[test]
    fn dead_connection_with_known_route_is_loss_at_every_hop() {
        let mut st = known(3);
        let a = assemble(ip(T), &[lost(), lost(), lost()], &mut st);
        assert_eq!(a.hops.len(), 3);
        assert!(a.target_known, "the last Hop is still the Target's position");
        assert!(a.hops.iter().all(|h| h.rtt_us.is_none()));
        assert_eq!(st.length, Some(3), "a silent Target keeps the known length");
    }

    #[test]
    fn router_unreachable_is_loss_not_a_reply() {
        // The home router's internet link is down: it answers every TTL >= 2
        // with "network unreachable".
        let mut st = known(4);
        let gw = "192.168.1.1";
        let a = assemble(ip(T), &[hop(gw, 1), unreachable(gw), unreachable(gw), unreachable(gw)], &mut st);
        assert_eq!(a.hops.len(), 4);
        assert!(a.target_known);
        assert_eq!(a.hops[0].address, Some(ip(gw)));
        assert!(a.hops[1..].iter().all(|h| h.address.is_none() && h.rtt_us.is_none()), "{:?}", a.hops);
        assert!(a.hops[1..].iter().all(|h| h.unreachable_from == Some(ip(gw))), "the router that said so is noted");
        assert_eq!(a.hops[0].unreachable_from, None);
    }

    #[test]
    fn router_saying_unreachable_for_its_own_hop_answered_that_hop() {
        // A Linux-based home router with its internet link down answers even
        // TTL 1 with "network unreachable" (it looks up the route before the
        // TTL). It still answered, so only the Hops beyond it are lost.
        let gw = "192.168.1.1";
        let mut st = known(3);
        assemble(ip(T), &[hop(gw, 1), hop("10.0.0.2", 5), echo(T, 9)], &mut st);
        let a = assemble(ip(T), &[unreachable(gw), unreachable(gw), unreachable(gw)], &mut st);
        assert_eq!(a.hops[0], HopResult { hop: 1, address: Some(ip(gw)), rtt_us: Some(1000), unreachable_from: None });
        assert!(a.hops[1..].iter().all(|h| h.rtt_us.is_none() && h.unreachable_from == Some(ip(gw))), "{:?}", a.hops);
        assert!(a.target_known);
        assert_eq!(st.length, Some(3));
    }

    #[test]
    fn target_unreachable_counts_as_reached() {
        let mut st = RouteState::default();
        let a = assemble(ip(T), &[hop("10.0.0.1", 1), unreachable(T)], &mut st);
        assert_eq!(a.hops[1].address, Some(ip(T)));
        assert_eq!(st.length, Some(2));
    }

    #[test]
    fn route_that_grew_is_rediscovered() {
        let mut st = known(2);
        let a = assemble(ip(T), &[hop("10.0.0.1", 1), hop("10.0.0.2", 2)], &mut st);
        assert_eq!(a.hops.len(), 2);
        assert!(!a.target_known);
        assert_eq!(st.length, None);
    }

    #[test]
    fn unknown_route_records_up_to_last_responder_and_probes_a_little_past_it() {
        let mut st = RouteState::default();
        assert_eq!(st.ttls_to_probe(), MAX_HOPS);
        let a = assemble(ip(T), &[hop("10.0.0.1", 1), lost(), hop("10.0.0.3", 3), lost(), lost()], &mut st);
        assert_eq!(a.hops.len(), 3);
        assert!(!a.target_known);
        assert_eq!(st.ttls_to_probe(), 3 + PROBE_PAST_LAST_RESPONDER);
        // Periodically everything is probed again.
        for _ in 1..FULL_PROBE_EVERY {
            st.ttls_to_probe();
        }
        assert_eq!(st.ttls_to_probe(), MAX_HOPS);

        let a = assemble(ip(T), &[lost(), lost()], &mut RouteState::default());
        assert_eq!(a.hops.len(), 1, "nothing answered: still record the first Hop as lost");
    }

    #[test]
    fn probe_errors_count_as_lost() {
        let mut st = known(2);
        let a = assemble(ip(T), &[Err(io::Error::other("net down")), echo(T, 5)], &mut st);
        assert_eq!(a.hops[0].rtt_us, None);
    }

    fn fake_route(target: IpAddr, ttl: u8, _: Duration) -> io::Result<ProbeOutcome> {
        Ok(if ttl >= 3 {
            ProbeOutcome::Echo { from: target, rtt: Duration::from_millis(5) }
        } else {
            ProbeOutcome::TimeExceeded { from: IpAddr::from([10, 0, 0, ttl]), rtt: Duration::from_millis(1) }
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn session_samples_on_interval_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let file = store.create(T, ip(T), 50, 100).unwrap();
        let id = file.id().to_string();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = count.clone();
        let config = SessionConfig { target: ip(T), interval_ms: 50, timeout: Duration::from_millis(100) };
        let (control, handle) = start(file, config, Arc::new(fake_route), move |_| {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        tokio::time::sleep(Duration::from_millis(330)).await;
        control.stop();
        handle.await.unwrap().unwrap();

        let file = store.session(&id).unwrap();
        let samples = file.samples(0, i64::MAX).unwrap();
        assert!((5..=8).contains(&samples.len()), "got {} samples", samples.len());
        assert_eq!(samples.len(), count.load(std::sync::atomic::Ordering::SeqCst));
        assert!(samples.iter().all(|s| s.hops.len() == 3 && s.target_known));
        assert!(file.meta().unwrap().stopped_at.is_some());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn write_failure_stops_the_session_with_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let file = store.create(T, ip(T), 20, 50).unwrap();
        let id = file.id().to_string();
        // Make every insert fail.
        store.session(&id).unwrap().break_for_tests();
        let config = SessionConfig { target: ip(T), interval_ms: 20, timeout: Duration::from_millis(50) };
        let (_control, handle) = start(file, config, Arc::new(fake_route), |_| {});
        let result = tokio::time::timeout(Duration::from_secs(5), handle).await.expect("session should end").unwrap();
        assert!(result.is_err());
        assert!(store.session(&id).unwrap().meta().unwrap().stopped_at.is_some(), "stop time still recorded");
    }
}
