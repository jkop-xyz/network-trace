use crate::settings::Settings;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use trace_core::engine::{self, SessionConfig, SessionControl};
use trace_core::model::SessionId;
use trace_core::store::Store;

pub struct Running {
    pub control: SessionControl,
    pub target: String,
    pub address: IpAddr,
    pub done: tokio::sync::watch::Receiver<bool>,
}

pub struct AppState {
    pub app: AppHandle,
    pub store: Store,
    pub running: Mutex<HashMap<SessionId, Running>>,
    /// Why a Session stopped on its own, for Sessions that failed this run.
    pub errors: Mutex<HashMap<SessionId, String>>,
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    /// Reverse DNS names of Hop addresses; `None` = looked up, no name.
    pub names: Mutex<HashMap<IpAddr, Option<String>>>,
    /// The Report being printed, served to the print window by the
    /// `ntreport` URI scheme so no file path ends up in printed footers.
    pub print_html: Mutex<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    pub session_id: SessionId,
    pub error: Option<String>,
}

impl AppState {
    pub fn load(app: &AppHandle) -> Result<AppState> {
        let data = app.path().app_data_dir().context("no app data directory")?;
        let config = app.path().app_config_dir().context("no app config directory")?;
        let store = Store::open(data.join("sessions"))?;
        // Nothing runs yet (the single-instance plugin guarantees no other
        // copy of the app is recording), so any Session still marked
        // running was cut short.
        store.recover(&[])?;
        let settings_path = config.join("settings.json");
        Ok(AppState {
            app: app.clone(),
            store,
            running: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
            settings: Mutex::new(Settings::load(&settings_path)),
            settings_path,
            names: Mutex::new(HashMap::new()),
            print_html: Mutex::new(String::new()),
        })
    }

    /// The running Session already watching this Target, if any.
    pub fn running_for(&self, target: &str, address: IpAddr) -> Option<SessionId> {
        self.running
            .lock()
            .unwrap()
            .iter()
            .find(|(_, r)| r.address == address || r.target.eq_ignore_ascii_case(target))
            .map(|(id, _)| id.clone())
    }

    pub fn start(&self, target: &str, address: IpAddr, interval_ms: u32) -> Result<SessionId> {
        let timeout_ms = self.settings.lock().unwrap().timeout_ms;
        let file = self.store.create(target, address, interval_ms, timeout_ms)?;
        let id = file.id().to_string();
        let app = self.app.clone();
        let sample_id = id.clone();
        let (control, handle) = engine::start(
            file,
            SessionConfig { target: address, interval_ms, timeout: std::time::Duration::from_millis(timeout_ms as u64) },
            engine::icmp_prober(),
            move |_| {
                let _ = app.emit("session-sample", SessionEvent { session_id: sample_id.clone(), error: None });
            },
        );
        let (done_tx, done_rx) = tokio::sync::watch::channel(false);
        self.running.lock().unwrap().insert(
            id.clone(),
            Running { control, target: target.to_string(), address, done: done_rx },
        );
        self.update_tray();

        let app = self.app.clone();
        let finished_id = id.clone();
        tauri::async_runtime::spawn(async move {
            let result = handle.await;
            let state = app.state::<AppState>();
            state.running.lock().unwrap().remove(&finished_id);
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(format!("{e:#}")),
                Err(e) => Some(format!("stopped unexpectedly: {e}")),
            };
            if let Some(e) = &error {
                eprintln!("session {finished_id} ended with error: {e}");
                state.errors.lock().unwrap().insert(finished_id.clone(), e.clone());
            }
            state.update_tray();
            let _ = done_tx.send(true);
            let _ = app.emit("session-stopped", SessionEvent { session_id: finished_id, error });
        });
        Ok(id)
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.lock().unwrap().contains_key(id)
    }

    /// Stops a Session and waits until its stop time is recorded.
    pub async fn stop(&self, id: &str) {
        let done = {
            let running = self.running.lock().unwrap();
            let Some(r) = running.get(id) else { return };
            r.control.stop();
            r.done.clone()
        };
        let mut done = done;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(15), done.wait_for(|d| *d)).await;
    }

    pub async fn stop_all(&self) {
        let ids: Vec<SessionId> = self.running.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.stop(&id).await;
        }
    }

    /// Shows how many Sessions are running in the tray tooltip.
    pub fn update_tray(&self) {
        let n = self.running.lock().unwrap().len();
        if let Some(tray) = self.app.tray_by_id("main") {
            let tip = match n {
                0 => "Network Trace: no Sessions running".to_string(),
                1 => "Network Trace: 1 Session running".to_string(),
                n => format!("Network Trace: {n} Sessions running"),
            };
            let _ = tray.set_tooltip(Some(tip));
        }
    }
}
