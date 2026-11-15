//! Statistics over a Time Window of a Session: per-Hop figures, timeline
//! buckets, Gaps and Route Changes. These feed both the screen and Reports.

use crate::model::Millis;
use crate::store::SessionFile;
use anyhow::Result;
use rusqlite::params;
use serde::Serialize;
use std::collections::BTreeMap;
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddressSeen {
    pub address: IpAddr,
    pub count: u32,
    pub first: Millis,
    pub last: Millis,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HopStats {
    pub hop: u8,
    /// The address that answered most often at this Hop.
    pub address: Option<IpAddr>,
    /// Every address that answered here, most frequent first.
    pub addresses: Vec<AddressSeen>,
    pub sent: u32,
    pub lost: u32,
    pub loss_pct: f64,
    pub min_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Round-trip time in the latest Sample of the window.
    pub cur_ms: Option<f64>,
    /// Mean absolute difference between consecutive round-trip times.
    pub jitter_ms: Option<f64>,
}

fn ms(us: Option<f64>) -> Option<f64> {
    us.map(|v| v / 1000.0)
}

/// Per-Hop statistics for Samples in `from..=to`.
pub fn hop_stats(file: &SessionFile, from: Millis, to: Millis) -> Result<Vec<HopStats>> {
    let conn = file.conn();
    let mut by_hop: BTreeMap<u8, HopStats> = BTreeMap::new();

    let mut stmt = conn.prepare_cached(
        "SELECT p.hop, COUNT(*), SUM(p.rtt_us IS NULL), MIN(p.rtt_us), AVG(p.rtt_us), MAX(p.rtt_us)
         FROM probes p JOIN samples s ON s.id = p.sample_id
         WHERE s.ts BETWEEN ?1 AND ?2 GROUP BY p.hop",
    )?;
    let rows = stmt.query_map(params![from, to], |r| {
        let sent: u32 = r.get(1)?;
        let lost: u32 = r.get(2)?;
        Ok(HopStats {
            hop: r.get(0)?,
            address: None,
            addresses: Vec::new(),
            sent,
            lost,
            loss_pct: if sent == 0 { 0.0 } else { lost as f64 * 100.0 / sent as f64 },
            min_ms: ms(r.get::<_, Option<i64>>(3)?.map(|v| v as f64)),
            avg_ms: ms(r.get(4)?),
            max_ms: ms(r.get::<_, Option<i64>>(5)?.map(|v| v as f64)),
            cur_ms: None,
            jitter_ms: None,
        })
    })?;
    for row in rows {
        let s = row?;
        by_hop.insert(s.hop, s);
    }

    let mut stmt = conn.prepare_cached(
        "SELECT p.hop, p.address, COUNT(*), MIN(s.ts), MAX(s.ts)
         FROM probes p JOIN samples s ON s.id = p.sample_id
         WHERE s.ts BETWEEN ?1 AND ?2 AND p.address IS NOT NULL
         GROUP BY p.hop, p.address ORDER BY p.hop, COUNT(*) DESC",
    )?;
    let rows = stmt.query_map(params![from, to], |r| {
        Ok((r.get::<_, u8>(0)?, r.get::<_, String>(1)?, r.get::<_, u32>(2)?, r.get(3)?, r.get(4)?))
    })?;
    for row in rows {
        let (hop, address, count, first, last) = row?;
        let (Some(stats), Ok(address)) = (by_hop.get_mut(&hop), address.parse()) else { continue };
        stats.addresses.push(AddressSeen { address, count, first, last });
    }

    let mut stmt = conn.prepare_cached(
        "SELECT p.hop, p.rtt_us FROM probes p
         WHERE p.sample_id = (SELECT id FROM samples WHERE ts BETWEEN ?1 AND ?2 ORDER BY ts DESC, id DESC LIMIT 1)",
    )?;
    let rows = stmt.query_map(params![from, to], |r| Ok((r.get::<_, u8>(0)?, r.get::<_, Option<i64>>(1)?)))?;
    for row in rows {
        let (hop, rtt) = row?;
        if let Some(stats) = by_hop.get_mut(&hop) {
            stats.cur_ms = ms(rtt.map(|v| v as f64));
        }
    }

    let mut stmt = conn.prepare_cached(
        "SELECT hop, AVG(ABS(rtt - prev)) FROM (
             SELECT p.hop AS hop, p.rtt_us AS rtt,
                    LAG(p.rtt_us) OVER (PARTITION BY p.hop ORDER BY s.ts, s.id) AS prev
             FROM probes p JOIN samples s ON s.id = p.sample_id
             WHERE s.ts BETWEEN ?1 AND ?2)
         WHERE rtt IS NOT NULL AND prev IS NOT NULL GROUP BY hop",
    )?;
    let rows = stmt.query_map(params![from, to], |r| Ok((r.get::<_, u8>(0)?, r.get::<_, Option<f64>>(1)?)))?;
    for row in rows {
        let (hop, jitter) = row?;
        if let Some(stats) = by_hop.get_mut(&hop) {
            stats.jitter_ms = ms(jitter);
        }
    }

    let mut out: Vec<HopStats> = by_hop.into_values().collect();
    for s in &mut out {
        s.address = s.addresses.first().map(|a| a.address);
    }
    Ok(out)
}

/// Figures for the Target itself: the last Hop of every Sample in which the
/// Target's position was known. `None` if the Target never answered in the
/// window, so the last recorded Hop was some router.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TargetStats {
    pub sent: u32,
    pub lost: u32,
    pub loss_pct: f64,
    pub min_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    /// Hop number of the Target in the latest such Sample.
    pub hop: u8,
}

pub fn target_stats(file: &SessionFile, from: Millis, to: Millis) -> Result<Option<TargetStats>> {
    let conn = file.conn();
    let mut stmt = conn.prepare_cached(
        "WITH t AS (
             SELECT s.ts AS ts, s.id AS sid, p.hop AS hop, p.rtt_us AS rtt
             FROM samples s JOIN probes p ON p.sample_id = s.id
             WHERE s.ts BETWEEN ?1 AND ?2 AND s.target_known = 1
               AND p.hop = (SELECT MAX(q.hop) FROM probes q WHERE q.sample_id = s.id)),
         j AS (SELECT rtt, LAG(rtt) OVER (ORDER BY ts, sid) AS prev FROM t)
         SELECT COUNT(*), SUM(rtt IS NULL), MIN(rtt), AVG(rtt), MAX(rtt),
                (SELECT AVG(ABS(rtt - prev)) FROM j WHERE rtt IS NOT NULL AND prev IS NOT NULL),
                (SELECT hop FROM t ORDER BY ts DESC, sid DESC LIMIT 1)
         FROM t",
    )?;
    let row = stmt.query_row(params![from, to], |r| {
        let sent: u32 = r.get(0)?;
        Ok((sent, r.get::<_, Option<u32>>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, Option<f64>>(3)?, r.get::<_, Option<i64>>(4)?, r.get::<_, Option<f64>>(5)?, r.get::<_, Option<u8>>(6)?))
    })?;
    let (sent, lost, min, avg, max, jitter, hop) = row;
    if sent == 0 {
        return Ok(None);
    }
    let lost = lost.unwrap_or(0);
    Ok(Some(TargetStats {
        sent,
        lost,
        loss_pct: lost as f64 * 100.0 / sent as f64,
        min_ms: ms(min.map(|v| v as f64)),
        avg_ms: ms(avg),
        max_ms: ms(max.map(|v| v as f64)),
        jitter_ms: ms(jitter),
        hop: hop.unwrap_or(0),
    }))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub start: Millis,
    pub end: Millis,
    /// Probes in this bucket; zero means no data (a Gap or outside the Session).
    pub sent: u32,
    pub lost: u32,
    pub min_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

/// Which Hop a timeline is for.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "hop")]
pub enum TimelineHop {
    /// The last recorded Hop of each Sample, i.e. the Target.
    Final,
    Number(u8),
}

/// Splits `from..=to` into `buckets` equal slices and aggregates one Hop's
/// Probes in each.
pub fn timeline(file: &SessionFile, hop: TimelineHop, from: Millis, to: Millis, buckets: u32) -> Result<Vec<Bucket>> {
    let buckets = buckets.max(1) as i64;
    let span = (to - from).max(1) + 1;
    let hop_filter = match hop {
        TimelineHop::Final => {
            "s.target_known = 1 AND p.hop = (SELECT MAX(q.hop) FROM probes q WHERE q.sample_id = p.sample_id)".to_string()
        }
        TimelineHop::Number(n) => format!("p.hop = {n}"),
    };
    let sql = format!(
        "SELECT ((s.ts - ?1) * ?3) / ?4 AS b, COUNT(*), SUM(p.rtt_us IS NULL), MIN(p.rtt_us), AVG(p.rtt_us), MAX(p.rtt_us)
         FROM probes p JOIN samples s ON s.id = p.sample_id
         WHERE s.ts BETWEEN ?1 AND ?2 AND {hop_filter} GROUP BY b"
    );
    let conn = file.conn();
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut out: Vec<Bucket> = (0..buckets)
        .map(|i| Bucket {
            start: from + span * i / buckets,
            end: from + span * (i + 1) / buckets - 1,
            sent: 0,
            lost: 0,
            min_ms: None,
            avg_ms: None,
            max_ms: None,
        })
        .collect();
    let rows = stmt.query_map(params![from, to, buckets, span], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<f64>>(4)?,
            r.get::<_, Option<i64>>(5)?,
        ))
    })?;
    for row in rows {
        let (b, sent, lost, min, avg, max) = row?;
        if let Some(bucket) = out.get_mut(b as usize) {
            bucket.sent = sent;
            bucket.lost = lost;
            bucket.min_ms = ms(min.map(|v| v as f64));
            bucket.avg_ms = ms(avg);
            bucket.max_ms = ms(max.map(|v| v as f64));
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Gap {
    /// Time of the last Sample before the Gap.
    pub start: Millis,
    /// Time of the first Sample after it.
    pub end: Millis,
}

/// Stretches with no Samples: consecutive Samples further apart than three
/// intervals plus a second.
pub fn gaps(file: &SessionFile, from: Millis, to: Millis) -> Result<Vec<Gap>> {
    let times = file.sample_times(from, to)?;
    Ok(times
        .windows(2)
        // Use the longer of the two intervals so changing the interval
        // (say from 1 s to 60 s) doesn't look like a Gap.
        .filter(|w| w[1].0 - w[0].0 > 3 * w[0].1.max(w[1].1) as i64 + 1000)
        .map(|w| Gap { start: w[0].0, end: w[1].0 })
        .collect())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HopChange {
    pub hop: u8,
    pub from: Option<IpAddr>,
    pub to: Option<IpAddr>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RouteChange {
    pub ts: Millis,
    pub changes: Vec<HopChange>,
}

/// Moments in the window where the Route differs from the one before.
///
/// The Route is the last address seen at each Hop; a lost Probe does not
/// change it. A different number of Hops to the Target is a change too.
pub fn route_changes(file: &SessionFile, from: Millis, to: Millis) -> Result<Vec<RouteChange>> {
    let samples = file.samples(from, to)?;
    let mut route: Vec<Option<IpAddr>> = Vec::new();
    let mut out = Vec::new();
    let mut started = false;
    for sample in samples {
        let mut changes = Vec::new();
        let len = sample.hops.len();
        if started && len != route.len() {
            // Hops that appeared or disappeared at the end of the route.
            for hop in len.min(route.len())..len.max(route.len()) {
                let from = route.get(hop).copied().flatten();
                let to = sample.hops.get(hop).and_then(|h| h.address);
                changes.push(HopChange { hop: hop as u8 + 1, from, to });
            }
        }
        route.resize(len, None);
        for h in &sample.hops {
            let i = h.hop as usize - 1;
            if let Some(addr) = h.address {
                if started && route[i].is_some() && route[i] != Some(addr) {
                    changes.push(HopChange { hop: h.hop, from: route[i], to: Some(addr) });
                }
                route[i] = Some(addr);
            }
        }
        changes.sort_by_key(|c| c.hop);
        changes.dedup_by_key(|c| c.hop);
        if !changes.is_empty() {
            out.push(RouteChange { ts: sample.ts, changes });
        }
        started = true;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HopResult, Sample};
    use crate::store::Store;

    fn a(last: u8) -> Option<IpAddr> {
        Some(IpAddr::from([10, 0, 0, last]))
    }
    fn sample(ts: Millis, hops: &[(Option<IpAddr>, Option<u32>)]) -> Sample {
        Sample {
            ts,
            interval_ms: 1000,
            target_known: true,
            hops: hops
                .iter()
                .enumerate()
                .map(|(i, (address, rtt_us))| HopResult { hop: i as u8 + 1, address: *address, rtt_us: *rtt_us })
                .collect(),
        }
    }
    fn session(samples: &[Sample]) -> (tempfile::TempDir, SessionFile) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut file = store.create("t", "10.0.0.9".parse().unwrap(), 1000, 3000).unwrap();
        for s in samples {
            file.append(s).unwrap();
        }
        (dir, file)
    }

    #[test]
    fn hop_stats_aggregate_loss_latency_and_addresses() {
        let (_dir, file) = session(&[
            sample(1000, &[(a(1), Some(1000)), (a(9), Some(10_000))]),
            sample(2000, &[(a(1), Some(3000)), (None, None)]),
            sample(3000, &[(a(2), Some(2000)), (a(9), Some(12_000))]),
            sample(9000, &[(a(1), Some(9000)), (a(9), Some(9000))]), // outside the window
        ]);
        let stats = hop_stats(&file, 0, 5000).unwrap();
        assert_eq!(stats.len(), 2);
        let h1 = &stats[0];
        assert_eq!((h1.sent, h1.lost), (3, 0));
        assert_eq!((h1.min_ms, h1.avg_ms, h1.max_ms, h1.cur_ms), (Some(1.0), Some(2.0), Some(3.0), Some(2.0)));
        assert_eq!(h1.address, a(1));
        assert_eq!(h1.addresses.len(), 2);
        assert_eq!(h1.jitter_ms, Some(1.5)); // |3-1| and |2-3|
        let h2 = &stats[1];
        assert_eq!((h2.sent, h2.lost), (3, 1));
        assert!((h2.loss_pct - 33.333).abs() < 0.01);
    }

    #[test]
    fn timeline_buckets_and_final_hop() {
        let (_dir, file) = session(&[
            sample(0, &[(a(1), Some(1000)), (a(9), Some(10_000))]),
            sample(1000, &[(a(1), Some(1000)), (None, None)]),
            // Route got longer: the final Hop is now hop 3.
            sample(3000, &[(a(1), Some(1000)), (a(5), Some(5000)), (a(9), Some(20_000))]),
        ]);
        let b = timeline(&file, TimelineHop::Final, 0, 3999, 4).unwrap();
        assert_eq!(b.len(), 4);
        assert_eq!((b[0].sent, b[0].avg_ms), (1, Some(10.0)));
        assert_eq!((b[1].sent, b[1].lost), (1, 1));
        assert_eq!(b[2].sent, 0, "no Samples in this slice");
        assert_eq!(b[3].avg_ms, Some(20.0));
        let b = timeline(&file, TimelineHop::Number(1), 0, 3999, 1).unwrap();
        assert_eq!(b[0].sent, 3);
    }

    #[test]
    fn target_stats_skip_samples_where_the_target_was_unknown() {
        let (_dir, file) = session(&[
            sample(0, &[(a(1), Some(1000)), (a(9), Some(10_000))]),
            sample(1000, &[(a(1), Some(1000)), (None, None)]),
        ]);
        let mut unknown = sample(2000, &[(a(1), Some(1000))]);
        unknown.target_known = false;
        let mut file = file;
        file.append(&unknown).unwrap();
        let t = target_stats(&file, 0, 5000).unwrap().unwrap();
        assert_eq!((t.sent, t.lost, t.hop), (2, 1, 2));
        assert_eq!(t.avg_ms, Some(10.0));
        let b = timeline(&file, TimelineHop::Final, 0, 2999, 3).unwrap();
        assert_eq!(b[2].sent, 0, "a Sample without a known Target isn't the Target");
    }

    #[test]
    fn interval_increase_is_not_a_gap() {
        let (_dir, mut file) = session(&[sample(0, &[]), sample(1000, &[])]);
        file.append(&Sample { ts: 61_000, interval_ms: 60_000, target_known: true, hops: vec![] }).unwrap();
        assert!(gaps(&file, 0, 100_000).unwrap().is_empty());
    }

    #[test]
    fn gaps_found_between_distant_samples() {
        let (_dir, file) = session(&[sample(0, &[]), sample(1000, &[]), sample(60_000, &[]), sample(61_000, &[])]);
        // Samples with no hops are not stored by `samples` joins but are in sample_times.
        assert_eq!(gaps(&file, 0, 100_000).unwrap(), vec![Gap { start: 1000, end: 60_000 }]);
    }

    #[test]
    fn route_changes_ignore_loss_and_catch_new_addresses_and_lengths() {
        let (_dir, file) = session(&[
            sample(0, &[(a(1), Some(1)), (a(2), Some(1)), (a(9), Some(1))]),
            sample(1000, &[(a(1), Some(1)), (None, None), (a(9), Some(1))]),
            sample(2000, &[(a(1), Some(1)), (a(3), Some(1)), (a(9), Some(1))]),
            sample(3000, &[(a(1), Some(1)), (a(3), Some(1)), (a(4), Some(1)), (a(9), Some(1))]),
        ]);
        let changes = route_changes(&file, 0, 10_000).unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0], RouteChange { ts: 2000, changes: vec![HopChange { hop: 2, from: a(2), to: a(3) }] });
        assert_eq!(changes[1].ts, 3000);
        assert_eq!(
            changes[1].changes,
            vec![HopChange { hop: 3, from: a(9), to: a(4) }, HopChange { hop: 4, from: None, to: a(9) }]
        );
    }
}
