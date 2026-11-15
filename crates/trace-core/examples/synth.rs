//! Writes a synthetic Session for trying out the UI and Reports without a
//! real network problem: `cargo run --example synth -- <sessions dir>`.
//!
//! Three hours to `203.0.113.10` (a documentation address) at 2.5 s, with:
//! - loss starting at hop 4 and carrying through to the Target for 15 minutes,
//! - hop 2 rate-limiting its replies (loss that does *not* carry through),
//! - a 20-minute Gap (machine asleep),
//! - a Route Change at hop 3 halfway through.

use std::net::IpAddr;
use trace_core::model::{HopResult, Sample, now_ms};
use trace_core::store::Store;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("sessions directory");
    let store = Store::open(dir)?;
    let target: IpAddr = "203.0.113.10".parse()?;
    let interval = 2500i64;
    let end = now_ms() - 60_000;
    let start = end - 3 * 3600 * 1000;
    let mut file = store.create_at(start, "synthetic.example", target, 2500, 3000)?;
    let hop_addr = |hop: u8, t: i64| -> IpAddr {
        match hop {
            1 => "192.168.1.1".parse().unwrap(),
            2 => "100.64.0.1".parse().unwrap(),
            3 if t > start + 90 * 60_000 => "198.51.100.33".parse().unwrap(),
            3 => "198.51.100.3".parse().unwrap(),
            4 => "198.51.100.4".parse().unwrap(),
            5 => "198.51.100.5".parse().unwrap(),
            _ => target,
        }
    };
    let base_ms = [0.4, 4.0, 6.0, 9.0, 11.0, 12.0];

    let mut rng = 0x2545F4914F6CDD1Du64;
    let mut rand = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng % 10_000) as f64 / 10_000.0
    };

    let outage = (start + 120 * 60_000, start + 135 * 60_000);
    let gap = (start + 40 * 60_000, start + 60 * 60_000);
    let mut t = start;
    let mut sample_id = 0u64;
    let tx_batch = 2000;
    while t < end {
        if !(gap.0..gap.1).contains(&t) {
            let in_outage = (outage.0..outage.1).contains(&t);
            let hops = (1..=6u8)
                .map(|hop| {
                    let i = hop as usize - 1;
                    let lost = (hop == 2 && rand() < 0.12) || (hop >= 4 && in_outage && rand() < 0.35) || rand() < 0.002;
                    let congestion = if hop >= 4 && in_outage { 25.0 + rand() * 40.0 } else { 0.0 };
                    let rtt = base_ms[i] + rand() * 1.5 + congestion;
                    HopResult {
                        hop,
                        address: (!lost).then(|| hop_addr(hop, t)),
                        rtt_us: (!lost).then(|| (rtt * 1000.0) as u32),
                    }
                })
                .collect();
            file.append(&Sample { ts: t, interval_ms: 2500, target_known: true, hops })?;
            sample_id += 1;
            if sample_id % tx_batch == 0 {
                eprint!(".");
            }
        }
        t += interval;
    }
    file.set_note("Synthetic data: loss at hop 4 onwards for 15 minutes, a 20 minute gap, and a route change at hop 3.")?;
    file.stop_at(end)?;
    eprintln!("\nwrote {} samples to session {}", sample_id, file.id());
    Ok(())
}
