//! Raw Export: every Probe in a Time Window as CSV.

use crate::model::Millis;
use crate::store::SessionFile;
use anyhow::Result;
use chrono::{Local, TimeZone};
use std::io::Write;

pub const CSV_HEADER: &str = "timestamp,hop,address,rtt_ms,lost";

/// ISO 8601 with milliseconds and the machine's UTC offset, e.g.
/// `2026-11-15T20:14:02.500+01:00`.
pub fn iso_local(ts: Millis) -> String {
    match Local.timestamp_millis_opt(ts) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => {
            t.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string()
        }
        chrono::LocalResult::None => ts.to_string(),
    }
}

/// Writes one row per Probe in `from..=to`, oldest first. Returns the row count.
pub fn write_csv(file: &SessionFile, from: Millis, to: Millis, out: &mut impl Write) -> Result<u64> {
    writeln!(out, "{CSV_HEADER}")?;
    let mut rows = 0;
    let (Some(first), Some(last)) = file.sample_range()? else { return Ok(0) };
    let (from, to) = (from.max(first), to.min(last));
    // Page through the window so huge Sessions don't sit in memory at once.
    let mut cursor = from;
    const PAGE: Millis = 60 * 60 * 1000;
    while cursor <= to {
        let page_end = (cursor + PAGE - 1).min(to);
        for sample in file.samples(cursor, page_end)? {
            let ts = iso_local(sample.ts);
            for h in &sample.hops {
                let address = h.address.map(|a| a.to_string()).unwrap_or_default();
                let rtt = h.rtt_us.map(|us| format!("{:.3}", us as f64 / 1000.0)).unwrap_or_default();
                let lost = if h.rtt_us.is_none() { 1 } else { 0 };
                writeln!(out, "{ts},{},{address},{rtt},{lost}", h.hop)?;
                rows += 1;
            }
        }
        cursor = page_end + 1;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HopResult, Sample};
    use crate::store::Store;

    #[test]
    fn csv_has_one_row_per_probe() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut file = store.create("t", "1.1.1.1".parse().unwrap(), 1000, 3000).unwrap();
        file.append(&Sample {
            ts: 1_700_000_000_000,
            interval_ms: 1000,
            hops: vec![
                HopResult { hop: 1, address: Some("10.0.0.1".parse().unwrap()), rtt_us: Some(1234) },
                HopResult { hop: 2, address: None, rtt_us: None },
            ],
        })
        .unwrap();
        let mut buf = Vec::new();
        assert_eq!(write_csv(&file, 0, i64::MAX / 2, &mut buf).unwrap(), 2);
        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], CSV_HEADER);
        assert!(lines[1].ends_with(",1,10.0.0.1,1.234,0"), "{}", lines[1]);
        assert!(lines[2].ends_with(",2,,,1"), "{}", lines[2]);
        // Offset-tagged timestamp: date, time with millis, then ±hh:mm.
        let ts = lines[1].split(',').next().unwrap();
        assert_eq!(ts.len(), "2023-11-14T22:13:20.000+00:00".len(), "{ts}");
    }
}
