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

pub type Prober = dyn Fn(IpAddr, u8, Duration) -> io::Result<ProbeOutcome> + Send + Sync;

/// The real prober: ICMP on the current platform.
pub fn icmp_prober() -> Arc<Prober> {
    Arc::new(probe)
}

/// Remembers how long the route to the Target is, so later Samples only
/// probe that far.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RouteLength(pub Option<u8>);

/// Turns one Sample's Probe outcomes (indexed by TTL, starting at 1) into the
/// Hops to record, and updates what is known about the route length.
///
/// - Target answered at TTL `n`: record Hops 1..=n; the route is `n` long.
/// - Target silent but the route length is known: record 1..=known, so a dead
///   connection shows as loss at every Hop including the final one. If a router
///   now answers "TTL exceeded" at the known final position, the route has grown
///   and is re-discovered next Sample.
/// - Nothing known yet: record up to the last Hop that answered.
pub fn assemble(target: IpAddr, outcomes: &[io::Result<ProbeOutcome>], length: &mut RouteLength) -> Vec<HopResult> {
    let reached = outcomes.iter().position(|o| match o {
        Ok(ProbeOutcome::Echo { .. }) => true,
        Ok(ProbeOutcome::Unreachable { from, .. }) => *from == target,
        _ => false,
    });

    let count = match (reached, length.0) {
        (Some(i), _) => {
            length.0 = Some(i as u8 + 1);
            i + 1
        }
        (None, Some(known)) => {
            let known = (known as usize).min(outcomes.len());
            if let Some(Ok(ProbeOutcome::TimeExceeded { .. })) = outcomes.get(known.wrapping_sub(1)) {
                length.0 = None;
            }
            known
        }
        (None, None) => outcomes
            .iter()
            .rposition(|o| matches!(o, Ok(o) if o.responder().is_some()))
            .map(|i| i + 1)
            .unwrap_or(1)
            .min(outcomes.len()),
    };

    outcomes[..count]
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let responder = o.as_ref().ok().and_then(|o| o.responder());
            HopResult {
                hop: i as u8 + 1,
                address: responder.map(|(a, _)| a),
                rtt_us: responder.map(|(_, rtt)| rtt.as_micros().min(u32::MAX as u128) as u32),
            }
        })
        .collect()
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
/// stopped and its stop time is recorded.
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
    mut file: SessionFile,
    config: SessionConfig,
    prober: Arc<Prober>,
    mut interval_rx: watch::Receiver<u32>,
    mut stop_rx: watch::Receiver<bool>,
    on_sample: impl Fn(&Sample) + Send + 'static,
) -> anyhow::Result<()> {
    let target = config.target;
    let length = Arc::new(std::sync::Mutex::new(RouteLength::default()));
    let (tx, mut rx) = mpsc::unbounded_channel::<RawSample>();

    // A single writer owns the Session file and the route-length state.
    let writer_length = length.clone();
    let writer = tokio::task::spawn_blocking(move || -> anyhow::Result<SessionFile> {
        while let Some((ts, interval_ms, outcomes)) = rx.blocking_recv() {
            let hops = assemble(target, &outcomes, &mut writer_length.lock().unwrap());
            let sample = Sample { ts, interval_ms, hops };
            file.append(&sample)?;
            on_sample(&sample);
        }
        Ok(file)
    });

    let mut in_flight = JoinSet::new();
    let mut next = Instant::now();
    loop {
        if *stop_rx.borrow() {
            break;
        }
        let interval_ms = *interval_rx.borrow();
        let ttl_max = length.lock().unwrap().0.unwrap_or(MAX_HOPS);
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
        // Reap finished Samples so the set doesn't grow forever.
        while in_flight.try_join_next().is_some() {}

        next += Duration::from_millis(interval_ms as u64);
        let now = Instant::now();
        if next < now {
            // We fell far behind (e.g. the machine slept): restart the cadence.
            next = now;
        }
        tokio::select! {
            _ = sleep_until(next) => {}
            _ = stop_rx.changed() => {}
            _ = interval_rx.changed() => {
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
    let file = writer.await??;
    file.stop_at(now_ms())?;
    Ok(())
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
    fn lost() -> io::Result<ProbeOutcome> {
        Ok(ProbeOutcome::Timeout)
    }

    const T: &str = "1.1.1.1";

    #[test]
    fn target_reached_records_up_to_target_and_learns_length() {
        let mut len = RouteLength::default();
        let hops = assemble(ip(T), &[hop("10.0.0.1", 1), lost(), echo(T, 9), echo(T, 9), echo(T, 9)], &mut len);
        assert_eq!(hops.len(), 3);
        assert_eq!(hops[1], HopResult { hop: 2, address: None, rtt_us: None });
        assert_eq!(hops[2], HopResult { hop: 3, address: Some(ip(T)), rtt_us: Some(9000) });
        assert_eq!(len, RouteLength(Some(3)));
    }

    #[test]
    fn dead_connection_with_known_route_is_loss_at_every_hop() {
        let mut len = RouteLength(Some(3));
        let hops = assemble(ip(T), &[lost(), lost(), lost()], &mut len);
        assert_eq!(hops.len(), 3);
        assert!(hops.iter().all(|h| h.rtt_us.is_none()));
        assert_eq!(len, RouteLength(Some(3)), "a silent Target keeps the known length");
    }

    #[test]
    fn route_that_grew_is_rediscovered() {
        let mut len = RouteLength(Some(2));
        let hops = assemble(ip(T), &[hop("10.0.0.1", 1), hop("10.0.0.2", 2)], &mut len);
        assert_eq!(hops.len(), 2);
        assert_eq!(len, RouteLength(None));
    }

    #[test]
    fn unknown_route_records_up_to_last_responder() {
        let mut len = RouteLength::default();
        let hops = assemble(ip(T), &[hop("10.0.0.1", 1), lost(), hop("10.0.0.3", 3), lost(), lost()], &mut len);
        assert_eq!(hops.len(), 3);
        let hops = assemble(ip(T), &[lost(), lost()], &mut len);
        assert_eq!(hops.len(), 1, "nothing answered: still record the first Hop as lost");
    }

    #[test]
    fn probe_errors_count_as_lost() {
        let mut len = RouteLength(Some(2));
        let hops = assemble(ip(T), &[Err(io::Error::other("net down")), echo(T, 5)], &mut len);
        assert_eq!(hops[0].rtt_us, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn session_samples_on_interval_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let file = store.create(T, ip(T), 50, 100).unwrap();
        let id = file.id().to_string();
        let fake: Arc<Prober> = Arc::new(|target, ttl, _| {
            Ok(if ttl >= 3 {
                ProbeOutcome::Echo { from: target, rtt: Duration::from_millis(5) }
            } else {
                ProbeOutcome::TimeExceeded { from: IpAddr::from([10, 0, 0, ttl]), rtt: Duration::from_millis(1) }
            })
        });
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = count.clone();
        let (control, handle) = start(file, SessionConfig { target: ip(T), interval_ms: 50, timeout: Duration::from_millis(100) }, fake, move |_| {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        tokio::time::sleep(Duration::from_millis(330)).await;
        control.stop();
        handle.await.unwrap().unwrap();

        let file = store.session(&id).unwrap();
        let samples = file.samples(0, i64::MAX).unwrap();
        assert!((5..=8).contains(&samples.len()), "got {} samples", samples.len());
        assert_eq!(samples.len(), count.load(std::sync::atomic::Ordering::SeqCst));
        assert!(samples.iter().all(|s| s.hops.len() == 3));
        assert!(file.meta().unwrap().stopped_at.is_some());
    }
}
