//! Plain data shared between the engine, storage, stats and the UI.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Milliseconds since the Unix epoch, UTC.
pub type Millis = i64;

pub fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(0)
}

/// Identifies a Session; also the stem of its file on disk.
pub type SessionId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: SessionId,
    /// What the user typed: a hostname or an IP address.
    pub target: String,
    /// The address being probed, resolved once when the Session started.
    pub address: IpAddr,
    pub started_at: Millis,
    /// `None` while the Session runs (or if the app died; see `Store::recover`).
    pub stopped_at: Option<Millis>,
    pub interval_ms: u32,
    pub timeout_ms: u32,
    pub note: String,
}

/// The result of one Probe as stored: lost when `rtt_us` is `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HopResult {
    pub hop: u8,
    pub address: Option<IpAddr>,
    pub rtt_us: Option<u32>,
    /// For a lost Probe: a router further back answered "destination
    /// unreachable" for it, i.e. it had no route onward.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreachable_from: Option<IpAddr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub ts: Millis,
    /// The Sample interval in force when this Sample was sent.
    pub interval_ms: u32,
    /// True when the last Hop is the Target's position (answered or not);
    /// false while the route to the Target is unknown.
    pub target_known: bool,
    pub hops: Vec<HopResult>,
}
