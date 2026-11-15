//! Commands the UI invokes. Errors are returned as display strings.

use crate::settings::{MAX_INTERVAL_MS, MIN_INTERVAL_MS, Settings};
use crate::state::AppState;
use serde::Serialize;
use std::collections::HashSet;
use std::net::IpAddr;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use trace_core::model::{Millis, SessionId, SessionMeta};
use trace_core::stats::{self, Bucket, Gap, HopStats, RouteChange, TimelineHop};
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
    /// didn't answer in the latest Sample).
    status: &'static str,
}

#[tauri::command]
pub async fn sessions_list(state: State<'_, AppState>) -> CmdResult<Vec<SessionSummary>> {
    let store = state.store.clone();
    let running: HashSet<SessionId> = state.running.lock().unwrap().keys().cloned().collect();
    blocking(move || {
        let mut out = Vec::new();
        for stored in store.list()? {
            let is_running = running.contains(&stored.meta.id);
            let status = if !is_running {
                "stopped"
            } else {
                match store.session(&stored.meta.id)?.last_sample()? {
                    None => "waiting",
                    Some(s) if s.hops.last().is_some_and(|h| h.rtt_us.is_none()) => "loss",
                    Some(_) => "ok",
                }
            };
            out.push(SessionSummary { stored, running: is_running, status });
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
    let mut addrs = tokio::net::lookup_host((target, 0)).await.map_err(|e| format!("Could not resolve {target}: {e}"))?;
    addrs.next().map(|a| a.ip()).ok_or_else(|| format!("{target} has no addresses"))
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

/// Starts a new Session for the same Target as an old one.
#[tauri::command]
pub async fn session_restart(state: State<'_, AppState>, id: String) -> CmdResult<StartResult> {
    let meta = state.store.session(&id).and_then(|f| f.meta()).map_err(err)?;
    session_start(state, meta.target).await
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
    gaps: Vec<Gap>,
    route_changes: Vec<RouteChange>,
    /// Reverse DNS names for every address mentioned above.
    names: std::collections::HashMap<String, String>,
}

#[tauri::command]
pub async fn session_view(state: State<'_, AppState>, id: String, from: Millis, to: Millis) -> CmdResult<SessionView> {
    let store = state.store.clone();
    let vid = id.clone();
    let (meta, range, hops, gaps, route_changes) = blocking(move || {
        let file = store.session(&vid)?;
        Ok((
            file.meta()?,
            file.sample_range()?,
            stats::hop_stats(&file, from, to)?,
            stats::gaps(&file, from, to)?,
            stats::route_changes(&file, from, to)?,
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
        gaps,
        route_changes,
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
    #[cfg(target_os = "linux")]
    if let Ok(release) = std::fs::read_to_string("/etc/os-release") {
        if let Some(name) = release.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")) {
            return format!("{} ({})", name.trim_matches('"'), std::env::consts::ARCH);
        }
    }
    format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH)
}

#[tauri::command]
pub fn save_text(path: String, contents: String) -> CmdResult<()> {
    std::fs::write(&path, contents).map_err(|e| format!("Could not write {path}: {e}"))
}

/// Opens a Report in its own window and shows the system print dialog,
/// from which it can be saved as PDF.
#[tauri::command]
pub fn print_report(app: AppHandle, html: String) -> CmdResult<()> {
    let dir = app.path().app_cache_dir().map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    let path = dir.join("report-print.html");
    std::fs::write(&path, html).map_err(err)?;
    let url = tauri::Url::from_file_path(&path).map_err(|_| "bad report path".to_string())?;

    if let Some(old) = app.get_webview_window("report-print") {
        let _ = old.destroy();
    }
    WebviewWindowBuilder::new(&app, "report-print", WebviewUrl::External(url))
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

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<AppState>) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(state.store.dir().to_string_lossy(), None::<&str>).map_err(err)
}
