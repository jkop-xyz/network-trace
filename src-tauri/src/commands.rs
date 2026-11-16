//! Commands the UI invokes. Errors are returned as display strings.

use crate::settings::{MAX_INTERVAL_MS, MIN_INTERVAL_MS, Settings};
use crate::state::AppState;
use serde::Serialize;
use std::collections::HashSet;
use std::net::IpAddr;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use trace_core::model::{Millis, SessionId, SessionMeta};
use trace_core::stats::{self, Bucket, Gap, HopStats, Outage, RouteChange, TargetStats, TimelineHop};
use trace_core::store::StoredSession;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn settings_get(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn settings_set(state: State<AppState>, settings: Settings) -> CmdResult<Settings> {
    let settings = settings.clamped();
    settings.save(&state.settings_path).map_err(err)?;
    *state.settings.lock().unwrap() = settings.clone();
    Ok(settings)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    #[serde(flatten)]
    stored: StoredSession,
    running: bool,
    /// "stopped", "waiting" (no Samples yet), "ok" or "loss" (the Target
    /// missed at least 2 of the last 3 Samples).
    status: &'static str,
    /// Why the Session stopped on its own, if it failed this run.
    error: Option<String>,
}

#[tauri::command]
pub async fn sessions_list(state: State<'_, AppState>) -> CmdResult<Vec<SessionSummary>> {
    let store = state.store.clone();
    let running: HashSet<SessionId> = state.running.lock().unwrap().keys().cloned().collect();
    let errors = state.errors.lock().unwrap().clone();
    blocking(move || {
        let mut out = Vec::new();
        for stored in store.list()? {
            let is_running = running.contains(&stored.meta.id);
            let status = if !is_running {
                "stopped"
            } else {
                let recent = store.session(&stored.meta.id)?.recent_samples(3)?;
                let lost = recent
                    .iter()
                    .filter(|s| s.target_known && s.hops.last().is_some_and(|h| h.rtt_us.is_none()))
                    .count();
                match (recent.len(), lost) {
                    (0, _) => "waiting",
                    (_, n) if n >= 2 || (recent.len() == 1 && n == 1) => "loss",
                    _ => "ok",
                }
            };
            let error = errors.get(&stored.meta.id).cloned();
            out.push(SessionSummary { stored, running: is_running, status, error });
        }
        Ok(out)
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    id: SessionId,
    /// True when a Session for this Target was already running; `id` is it.
    already_running: bool,
}

async fn resolve(target: &str) -> CmdResult<IpAddr> {
    if let Ok(ip) = target.trim_matches(|c| c == '[' || c == ']').parse::<IpAddr>() {
        return Ok(ip);
    }
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((target, 0))
        .await
        .map_err(|e| format!("Could not resolve {target}: {e}"))?
        .map(|a| a.ip())
        .collect();
    // Prefer IPv4: a host with an AAAA record on a network with broken IPv6
    // would otherwise look like 100% loss. Type an IPv6 address to force it.
    addrs.iter().find(|a| a.is_ipv4()).or(addrs.first()).copied().ok_or_else(|| format!("{target} has no addresses"))
}

#[tauri::command]
pub async fn session_start(state: State<'_, AppState>, target: String) -> CmdResult<StartResult> {
    let target = target.trim().to_string();
    if target.is_empty() {
        return Err("Enter a hostname or IP address".into());
    }
    let address = resolve(&target).await?;
    if let Some(id) = state.running_for(&target, address) {
        return Ok(StartResult { id, already_running: true });
    }
    // Fail fast if this machine doesn't allow unprivileged ICMP.
    let check = tauri::async_runtime::spawn_blocking(move || {
        trace_core::probe::probe(address, 64, std::time::Duration::from_millis(500))
    })
    .await
    .map_err(err)?;
    if let Err(e) = check {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            return Err(e.to_string());
        }
    }
    let interval = state.settings.lock().unwrap().default_interval_ms;
    let id = state.start(&target, address, interval).map_err(err)?;
    Ok(StartResult { id, already_running: false })
}

#[tauri::command]
pub async fn session_stop(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.stop(&id).await;
    Ok(())
}

/// Continues a stopped Session in place. If another Session for the same
/// Target is already running, that one is returned instead.
#[tauri::command]
pub async fn session_resume(state: State<'_, AppState>, id: String) -> CmdResult<StartResult> {
    if state.is_running(&id) {
        return Ok(StartResult { id, already_running: true });
    }
    let meta = state.store.session(&id).and_then(|f| f.meta()).map_err(err)?;
    if let Some(other) = state.running_for(&meta.target, meta.address) {
        return Ok(StartResult { id: other, already_running: true });
    }
    let id = state.resume(&id).map_err(err)?;
    Ok(StartResult { id, already_running: false })
}

#[tauri::command]
pub fn session_set_interval(state: State<AppState>, id: String, interval_ms: u32) -> CmdResult<u32> {
    let interval_ms = interval_ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
    if let Some(r) = state.running.lock().unwrap().get(&id) {
        r.control.set_interval(interval_ms);
    }
    state.store.session(&id).and_then(|f| f.set_interval(interval_ms)).map_err(err)?;
    Ok(interval_ms)
}

#[tauri::command]
pub fn session_set_note(state: State<AppState>, id: String, note: String) -> CmdResult<()> {
    state.store.session(&id).and_then(|f| f.set_note(&note)).map_err(err)
}

#[tauri::command]
pub fn sessions_delete(state: State<AppState>, ids: Vec<String>) -> CmdResult<()> {
    for id in &ids {
        if state.is_running(id) {
            return Err("Stop running Sessions before deleting them".into());
        }
    }
    for id in &ids {
        state.store.delete(id).map_err(err)?;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HopRow {
    #[serde(flatten)]
    stats: HopStats,
    name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    meta: SessionMeta,
    running: bool,
    size_bytes: u64,
    first_sample: Option<Millis>,
    last_sample: Option<Millis>,
    hops: Vec<HopRow>,
    /// The Target's own figures; `None` if it never answered in the window.
    target: Option<TargetStats>,
    gaps: Vec<Gap>,
    route_changes: Vec<RouteChange>,
    outages: Vec<Outage>,
    /// Reverse DNS names for every address mentioned above.
    names: std::collections::HashMap<String, String>,
}

#[tauri::command]
pub async fn session_view(state: State<'_, AppState>, id: String, from: Millis, to: Millis) -> CmdResult<SessionView> {
    let store = state.store.clone();
    let vid = id.clone();
    let (meta, range, hops, target, gaps, route_changes, outages) = blocking(move || {
        let file = store.session(&vid)?;
        Ok((
            file.meta()?,
            file.sample_range()?,
            stats::hop_stats(&file, from, to)?,
            stats::target_stats(&file, from, to)?,
            stats::gaps(&file, from, to)?,
            stats::route_changes(&file, from, to)?,
            stats::outages(&file, from, to)?,
        ))
    })
    .await?;

    let mut addresses: HashSet<IpAddr> = HashSet::new();
    for h in &hops {
        addresses.extend(h.addresses.iter().map(|a| a.address));
    }
    for rc in &route_changes {
        for c in &rc.changes {
            addresses.extend(c.from.iter().chain(c.to.iter()).copied());
        }
    }
    for o in &outages {
        addresses.extend(o.last_address.iter().chain(o.unreachable_from.iter()).copied());
    }
    let names = names_for(&state, addresses);
    let hops = hops
        .into_iter()
        .map(|stats| {
            let name = stats.address.and_then(|a| names.get(&a.to_string()).cloned());
            HopRow { stats, name }
        })
        .collect();
    Ok(SessionView {
        running: state.is_running(&id),
        size_bytes: state.store.size(&id),
        meta,
        first_sample: range.0,
        last_sample: range.1,
        hops,
        target,
        gaps,
        route_changes,
        outages,
        names,
    })
}

/// Cached reverse DNS names; unknown addresses are looked up in the
/// background and a `names-updated` event fires when they are done.
fn names_for(state: &AppState, addresses: HashSet<IpAddr>) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut missing = Vec::new();
    {
        let mut cache = state.names.lock().unwrap();
        for a in addresses {
            match cache.get(&a) {
                Some(Some(name)) => {
                    out.insert(a.to_string(), name.clone());
                }
                Some(None) => {}
                None => {
                    cache.insert(a, None); // in progress; don't look up twice
                    missing.push(a);
                }
            }
        }
    }
    if !missing.is_empty() {
        let app = state.app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            for a in missing {
                let name = trace_core::reverse_name(a);
                app.state::<AppState>().names.lock().unwrap().insert(a, name);
            }
            let _ = app.emit("names-updated", ());
        });
    }
    out
}

#[tauri::command]
pub async fn session_timeline(
    state: State<'_, AppState>,
    id: String,
    hop: TimelineHop,
    from: Millis,
    to: Millis,
    buckets: u32,
) -> CmdResult<Vec<Bucket>> {
    let store = state.store.clone();
    blocking(move || stats::timeline(&store.session(&id)?, hop, from, to, buckets.clamp(1, 4000))).await
}

#[tauri::command]
pub async fn export_csv(state: State<'_, AppState>, id: String, from: Millis, to: Millis, path: String) -> CmdResult<u64> {
    check_extension(&path, &["csv"])?;
    let store = state.store.clone();
    blocking(move || {
        let file = store.session(&id)?;
        let mut out = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let rows = trace_core::export::write_csv(&file, from, to, &mut out)?;
        std::io::Write::flush(&mut out)?;
        Ok(rows)
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportContext {
    os: String,
    app_version: String,
    /// Present when enabled in settings and the lookup succeeded.
    public_ip: Option<String>,
    public_ip_error: Option<String>,
}

#[tauri::command]
pub async fn report_context(state: State<'_, AppState>, app: AppHandle) -> CmdResult<ReportContext> {
    let lookup = state.settings.lock().unwrap().public_ip_in_report;
    let (public_ip, public_ip_error) = if lookup {
        match tauri::async_runtime::spawn_blocking(public_ip).await.map_err(err)? {
            Ok(ip) => (Some(ip), None),
            Err(e) => (None, Some(e)),
        }
    } else {
        (None, None)
    };
    Ok(ReportContext { os: os_description(), app_version: app.package_info().version.to_string(), public_ip, public_ip_error })
}

fn public_ip() -> Result<String, String> {
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(5))).build().into();
    let body = agent
        .get("https://api.ipify.org")
        .call()
        .map_err(err)?
        .body_mut()
        .read_to_string()
        .map_err(err)?;
    let ip: IpAddr = body.trim().parse().map_err(|_| "unexpected answer from api.ipify.org".to_string())?;
    Ok(ip.to_string())
}

fn os_description() -> String {
    #[cfg(windows)]
    if let Some(name) = windows_version() {
        return name;
    }
    #[cfg(target_os = "linux")]
    if let Ok(release) = std::fs::read_to_string("/etc/os-release") {
        if let Some(name) = release.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")) {
            return format!("{} ({})", name.trim_matches('"'), std::env::consts::ARCH);
        }
    }
    format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH)
}

/// The Windows product and version from the registry, e.g.
/// "Windows 11 Pro 24H2 (build 26100.2314, x86_64)".
#[cfg(windows)]
fn windows_version() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion").ok()?;
    let text = |name: &str| key.get_value::<String, _>(name).ok();
    let ubr = key.get_value::<u32, _>("UBR").ok();
    Some(describe_windows(
        &text("ProductName")?,
        text("InstallationType").as_deref(),
        text("CurrentBuild").as_deref(),
        text("DisplayVersion").as_deref(),
        ubr,
    ))
}

/// Builds a readable Windows description from registry values.
#[cfg_attr(not(windows), allow(dead_code))]
///
/// `ProductName` still says "Windows 10" on Windows 11, so desktop builds
/// from 22000 on are renamed. Server editions share build numbers with
/// Windows 11 but have the right `ProductName`, so they are left alone.
fn describe_windows(
    product: &str,
    installation_type: Option<&str>,
    build: Option<&str>,
    display_version: Option<&str>,
    ubr: Option<u32>,
) -> String {
    let build_no: u32 = build.and_then(|b| b.parse().ok()).unwrap_or(0);
    let is_server = installation_type.is_some_and(|t| t.eq_ignore_ascii_case("server")) || product.contains("Server");
    let mut name = product.trim().to_string();
    if !is_server && build_no >= 22000 && name.starts_with("Windows 10") {
        name = name.replacen("Windows 10", "Windows 11", 1);
    }
    if let Some(v) = display_version.filter(|v| !v.is_empty() && !is_server) {
        name = format!("{name} {v}");
    }
    let build = match (build, ubr) {
        (Some(b), Some(u)) => format!("build {b}.{u}"),
        (Some(b), None) => format!("build {b}"),
        _ => "unknown build".to_string(),
    };
    format!("{name} ({build}, {})", std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::describe_windows;

    #[test]
    fn windows_11_is_not_called_windows_10() {
        let s = describe_windows("Windows 10 Pro", Some("Client"), Some("26100"), Some("24H2"), Some(2314));
        assert!(s.starts_with("Windows 11 Pro 24H2 (build 26100.2314, "), "{s}");
    }

    #[test]
    fn windows_10_stays_windows_10() {
        let s = describe_windows("Windows 10 Home", Some("Client"), Some("19045"), Some("22H2"), Some(5011));
        assert!(s.starts_with("Windows 10 Home 22H2 (build 19045.5011, "), "{s}");
    }

    #[test]
    fn server_keeps_its_name_despite_a_windows_11_build() {
        let s = describe_windows("Windows Server 2025 Standard Evaluation", Some("Server"), Some("26100"), Some("24H2"), None);
        assert!(s.starts_with("Windows Server 2025 Standard Evaluation (build 26100, "), "{s}");
    }
}

/// Exports only ever write the file types the save dialog offers, which
/// limits what a compromised page could do with these commands.
fn check_extension(path: &str, allowed: &[&str]) -> CmdResult<()> {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if allowed.contains(&ext.as_str()) { Ok(()) } else { Err(format!("Can only save {} files", allowed.join("/"))) }
}

#[tauri::command]
pub fn save_text(path: String, contents: String) -> CmdResult<()> {
    check_extension(&path, &["html", "htm"])?;
    std::fs::write(&path, contents).map_err(|e| format!("Could not write {path}: {e}"))
}

/// Opens a Report in its own window and shows the system print dialog,
/// from which it can be saved as PDF.
///
/// Must stay `async`: a synchronous command that creates a window
/// deadlocks on Windows (WebView2), freezing the app (issue #1).
#[tauri::command]
pub async fn print_report(app: AppHandle, state: State<'_, AppState>, html: String) -> CmdResult<()> {
    // Served from memory by the ntreport scheme, so printed headers/footers
    // show a neutral URL rather than a path containing the user name.
    *state.print_html.lock().unwrap() = html;
    let url = if cfg!(windows) {
        format!("http://{}.localhost/report.html", crate::REPORT_SCHEME)
    } else {
        format!("{}://localhost/report.html", crate::REPORT_SCHEME)
    };
    let url = tauri::Url::parse(&url).map_err(err)?;

    if let Some(old) = app.get_webview_window("report-print") {
        let _ = old.destroy();
    }
    WebviewWindowBuilder::new(&app, "report-print", WebviewUrl::CustomProtocol(url))
        .title("Report — print or save as PDF")
        .inner_size(900.0, 1000.0)
        .on_page_load(|window, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                let _ = window.print();
            }
        })
        .build()
        .map_err(err)?;
    Ok(())
}

/// Opens the file manager with the file selected.
#[tauri::command]
pub fn reveal_file(app: AppHandle, path: String) -> CmdResult<()> {
    #[cfg(windows)]
    {
        // The shell API the opener plugin uses falls back to opening the
        // folder without a selection on some systems (issue #2); Explorer's
        // own /select switch reliably selects the file.
        use std::os::windows::process::CommandExt;
        let _ = &app;
        let path = std::path::Path::new(&path);
        if !path.exists() {
            return Err(format!("{} no longer exists", path.display()));
        }
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(err)
    }
    #[cfg(not(windows))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener().reveal_item_in_dir(path).map_err(err)
    }
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IpOwner {
    address: String,
    /// Network name from the registry, e.g. "CLOUDFLARENET".
    network: Option<String>,
    /// The organisation the block is registered to.
    organisation: Option<String>,
    country: Option<String>,
    /// The registered address block, e.g. "1.1.1.0 – 1.1.1.255".
    range: Option<String>,
    /// Link to the full registry record.
    source: String,
}

/// Who an address is registered to, from the public RDAP registries
/// (rdap.org redirects to the right one: RIPE, ARIN, APNIC, ...). Only runs
/// when the user asks for it.
#[tauri::command]
pub async fn ip_owner(address: String) -> CmdResult<IpOwner> {
    let ip: IpAddr = address.parse().map_err(|_| "not an IP address".to_string())?;
    if !is_public(ip) {
        return Ok(IpOwner {
            address,
            network: Some("Private or special-use address".into()),
            organisation: Some("Not registered publicly: part of a local network or reserved range".into()),
            source: String::new(),
            ..Default::default()
        });
    }
    let url = format!("https://rdap.org/ip/{ip}");
    let source = url.clone();
    let json: serde_json::Value = tauri::async_runtime::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .build()
            .into();
        let mut resp = agent.get(&url).header("Accept", "application/rdap+json").call().map_err(err)?;
        resp.body_mut().read_json().map_err(err)
    })
    .await
    .map_err(err)??;

    let s = |v: &serde_json::Value| v.as_str().map(str::to_string);
    let range = match (s(&json["startAddress"]), s(&json["endAddress"])) {
        (Some(a), Some(b)) => Some(format!("{a} – {b}")),
        _ => None,
    };
    Ok(IpOwner {
        address,
        network: s(&json["name"]),
        organisation: rdap_organisation(&json),
        country: s(&json["country"]),
        range,
        source,
    })
}

/// The organisation's name: an entity whose vCard kind is "org", then the
/// registrant, then any named entity (registries differ in what they fill in).
fn rdap_organisation(json: &serde_json::Value) -> Option<String> {
    let entities = json["entities"].as_array()?;
    let field = |e: &serde_json::Value, name: &str| -> Option<String> {
        e["vcardArray"][1].as_array()?.iter().find(|f| f[0] == name).and_then(|f| f[3].as_str()).map(str::to_string)
    };
    let has_role = |e: &serde_json::Value, role: &str| e["roles"].as_array().is_some_and(|r| r.iter().any(|x| x == role));
    entities
        .iter()
        .find(|e| field(e, "kind").as_deref() == Some("org"))
        .and_then(|e| field(e, "fn"))
        .or_else(|| entities.iter().find(|e| has_role(e, "registrant")).and_then(|e| field(e, "fn")))
        .or_else(|| entities.iter().find_map(|e| field(e, "fn")))
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_documentation()
                || v4.is_broadcast()
                || v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1])) // CGNAT
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !(v6.is_loopback() || v6.is_unspecified() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80)
        }
    }
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<AppState>) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(state.store.dir().to_string_lossy(), None::<&str>).map_err(err)
}
