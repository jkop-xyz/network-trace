//! Windows: the IP Helper ICMP API (`IcmpSendEcho2` / `Icmp6SendEcho2`),
//! which sends echo requests with a chosen TTL without admin rights.
//!
//! The declarations are written by hand to keep the FFI surface explicit.

use super::{PAYLOAD, ProbeOutcome};
use std::ffi::c_void;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

type Handle = *mut c_void;
const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const AF_INET6: u16 = 23;

const IP_SUCCESS: u32 = 0;
const IP_DEST_NET_UNREACHABLE: u32 = 11002;
const IP_DEST_HOST_UNREACHABLE: u32 = 11003;
const IP_DEST_PROT_UNREACHABLE: u32 = 11004;
const IP_DEST_PORT_UNREACHABLE: u32 = 11005;
const IP_REQ_TIMED_OUT: u32 = 11010;
const IP_TTL_EXPIRED_TRANSIT: u32 = 11013; // also IP_HOP_LIMIT_EXCEEDED for IPv6
const IP_TTL_EXPIRED_REASSEM: u32 = 11014;
const IP_TIME_EXCEEDED: u32 = 11041;
const IP_DEST_UNREACHABLE: u32 = 11040;

#[repr(C)]
struct IpOptionInformation {
    ttl: u8,
    tos: u8,
    flags: u8,
    options_size: u8,
    options_data: *mut u8,
}

#[repr(C)]
struct IcmpEchoReply {
    address: u32,
    status: u32,
    round_trip_time: u32,
    data_size: u16,
    reserved: u16,
    data: *mut c_void,
    options: IpOptionInformation,
}

#[repr(C)]
struct SockaddrIn6 {
    family: u16,
    port: u16,
    flowinfo: u32,
    addr: [u8; 16],
    scope_id: u32,
}

#[repr(C, packed)]
struct Ipv6AddressEx {
    port: u16,
    flowinfo: u32,
    addr: [u16; 8],
    scope_id: u32,
}

#[repr(C)]
struct Icmpv6EchoReply {
    address: Ipv6AddressEx,
    status: u32,
    round_trip_time: u32,
}

#[link(name = "iphlpapi")]
unsafe extern "system" {
    fn IcmpCreateFile() -> Handle;
    fn Icmp6CreateFile() -> Handle;
    fn IcmpCloseHandle(handle: Handle) -> i32;
    fn IcmpSendEcho2(
        handle: Handle,
        event: Handle,
        apc_routine: *const c_void,
        apc_context: *const c_void,
        destination: u32,
        request_data: *const c_void,
        request_size: u16,
        request_options: *const IpOptionInformation,
        reply_buffer: *mut c_void,
        reply_size: u32,
        timeout_ms: u32,
    ) -> u32;
    fn Icmp6SendEcho2(
        handle: Handle,
        event: Handle,
        apc_routine: *const c_void,
        apc_context: *const c_void,
        source: *const SockaddrIn6,
        destination: *const SockaddrIn6,
        request_data: *const c_void,
        request_size: u16,
        request_options: *const IpOptionInformation,
        reply_buffer: *mut c_void,
        reply_size: u32,
        timeout_ms: u32,
    ) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLastError() -> u32;
}

struct IcmpHandle(Handle);

impl Drop for IcmpHandle {
    fn drop(&mut self) {
        unsafe { IcmpCloseHandle(self.0) };
    }
}

pub fn probe(target: IpAddr, ttl: u8, timeout: Duration) -> io::Result<ProbeOutcome> {
    let timeout_ms = timeout.as_millis().clamp(1, u32::MAX as u128) as u32;
    let options = IpOptionInformation { ttl, tos: 0, flags: 0, options_size: 0, options_data: null_mut() };
    // u64 storage keeps the reply buffer aligned for the reply structs.
    let mut reply = [0u64; 128];
    let reply_size = std::mem::size_of_val(&reply) as u32;

    let start = Instant::now();
    match target {
        IpAddr::V4(v4) => {
            let handle = open(unsafe { IcmpCreateFile() })?;
            let n = unsafe {
                IcmpSendEcho2(
                    handle.0,
                    null_mut(),
                    null(),
                    null(),
                    u32::from_ne_bytes(v4.octets()),
                    PAYLOAD.as_ptr() as *const c_void,
                    PAYLOAD.len() as u16,
                    &options,
                    reply.as_mut_ptr() as *mut c_void,
                    reply_size,
                    timeout_ms,
                )
            };
            let rtt = start.elapsed();
            let r = unsafe { &*(reply.as_ptr() as *const IcmpEchoReply) };
            let status = if n == 0 { if r.address != 0 { r.status } else { unsafe { GetLastError() } } } else { r.status };
            let from = IpAddr::V4(Ipv4Addr::from(r.address.to_ne_bytes()));
            classify(status, from, rtt)
        }
        IpAddr::V6(v6) => {
            let handle = open(unsafe { Icmp6CreateFile() })?;
            let source = SockaddrIn6 { family: AF_INET6, port: 0, flowinfo: 0, addr: [0; 16], scope_id: 0 };
            let dest = SockaddrIn6 { family: AF_INET6, port: 0, flowinfo: 0, addr: v6.octets(), scope_id: 0 };
            let n = unsafe {
                Icmp6SendEcho2(
                    handle.0,
                    null_mut(),
                    null(),
                    null(),
                    &source,
                    &dest,
                    PAYLOAD.as_ptr() as *const c_void,
                    PAYLOAD.len() as u16,
                    &options,
                    reply.as_mut_ptr() as *mut c_void,
                    reply_size,
                    timeout_ms,
                )
            };
            let rtt = start.elapsed();
            let r = unsafe { std::ptr::read_unaligned(reply.as_ptr() as *const Icmpv6EchoReply) };
            let words = r.address.addr;
            let mut bytes = [0u8; 16];
            for (i, w) in words.iter().enumerate() {
                bytes[i * 2..i * 2 + 2].copy_from_slice(&w.to_ne_bytes());
            }
            let from = IpAddr::V6(Ipv6Addr::from(bytes));
            let status = if n == 0 && from.is_unspecified() { unsafe { GetLastError() } } else { r.status };
            classify(status, from, rtt)
        }
    }
}

fn open(handle: Handle) -> io::Result<IcmpHandle> {
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(IcmpHandle(handle))
    }
}

fn classify(status: u32, from: IpAddr, rtt: Duration) -> io::Result<ProbeOutcome> {
    Ok(match status {
        IP_SUCCESS => ProbeOutcome::Echo { from, rtt },
        IP_TTL_EXPIRED_TRANSIT | IP_TTL_EXPIRED_REASSEM | IP_TIME_EXCEEDED if !from.is_unspecified() => {
            ProbeOutcome::TimeExceeded { from, rtt }
        }
        IP_DEST_NET_UNREACHABLE
        | IP_DEST_HOST_UNREACHABLE
        | IP_DEST_PROT_UNREACHABLE
        | IP_DEST_PORT_UNREACHABLE
        | IP_DEST_UNREACHABLE
            if !from.is_unspecified() =>
        {
            ProbeOutcome::Unreachable { from, rtt }
        }
        IP_REQ_TIMED_OUT => ProbeOutcome::Timeout,
        // Any other status (no resources, general failure, ...) means the
        // Probe got no answer.
        _ => ProbeOutcome::Timeout,
    })
}
