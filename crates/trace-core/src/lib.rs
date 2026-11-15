//! The monitoring core of network-trace: Probes, Sessions, storage and the
//! statistics shown on screen and in Reports. Knows nothing about the UI.

pub mod engine;
pub mod export;
pub mod model;
pub mod probe;
pub mod stats;
pub mod store;

/// Reverse DNS name for a Hop's address, if it has one.
pub fn reverse_name(addr: std::net::IpAddr) -> Option<String> {
    dns_lookup::lookup_addr(&addr).ok().filter(|name| name != &addr.to_string())
}
