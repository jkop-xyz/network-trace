//! One-shot traceroute using the Probe primitive: `cargo run --example hops -- 1.1.1.1`
use std::time::Duration;
use trace_core::probe::{ProbeOutcome, probe};

fn main() {
    let target = std::env::args().nth(1).unwrap_or_else(|| "1.1.1.1".into()).parse().expect("IP address");
    for ttl in 1..=30u8 {
        let out = probe(target, ttl, Duration::from_secs(2));
        println!("{ttl:>2}  {out:?}");
        if matches!(out, Ok(ProbeOutcome::Echo { .. })) {
            break;
        }
    }
}
