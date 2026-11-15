use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// How long to wait before a Probe counts as lost.
    pub timeout_ms: u32,
    /// Sample interval for new Sessions.
    pub default_interval_ms: u32,
    /// Look up the public IP address when making a Report.
    pub public_ip_in_report: bool,
    /// The "still running in the tray" notice has been shown once.
    pub tray_notice_shown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { timeout_ms: 3000, default_interval_ms: 2500, public_ip_in_report: true, tray_notice_shown: false }
    }
}

pub const MIN_INTERVAL_MS: u32 = 1000;
pub const MAX_INTERVAL_MS: u32 = 60_000;

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str::<Settings>(&s).ok()).unwrap_or_default().clamped()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn clamped(mut self) -> Settings {
        self.timeout_ms = self.timeout_ms.clamp(200, 10_000);
        self.default_interval_ms = self.default_interval_ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
        self
    }
}
