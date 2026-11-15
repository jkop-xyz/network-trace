//! Linux desktop integration for the AppImage.
//!
//! On Wayland a window's icon (title bar, task bar) comes from the
//! `.desktop` file whose name matches the window's app id; the icon the app
//! sets itself is ignored. An AppImage has no installed `.desktop` file, so
//! the compositor shows a generic icon. When running as an AppImage, write a
//! user-level `.desktop` file and icon so the right icon appears (and the app
//! shows up in the application menu).

#[cfg(target_os = "linux")]
pub fn integrate() {
    if let Err(e) = linux::integrate() {
        eprintln!("desktop integration skipped: {e:#}");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn integrate() {}

#[cfg(target_os = "linux")]
mod linux {
    use anyhow::{Context, Result};
    use std::path::PathBuf;

    /// Must equal the Wayland app id of the window, which GTK takes from the
    /// program name (checked with WAYLAND_DEBUG: `set_app_id("network-trace")`).
    const APP_ID: &str = "network-trace";
    /// The entry beta.2–beta.5 wrote under the wrong name; removed if ours.
    const OLD_ENTRY: &str = "app.networktrace.desktop.desktop";
    const ICON_NAME: &str = "network-trace";
    const ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

    pub fn integrate() -> Result<()> {
        // Only for AppImages; packaged or dev builds are left alone.
        let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from) else { return Ok(()) };
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .context("no data directory")?;

        let icon_dir = data.join("icons/hicolor/128x128/apps");
        std::fs::create_dir_all(&icon_dir)?;
        let icon = icon_dir.join(format!("{ICON_NAME}.png"));
        if std::fs::read(&icon).ok().as_deref() != Some(ICON_PNG) {
            std::fs::write(&icon, ICON_PNG)?;
        }

        let apps = data.join("applications");
        std::fs::create_dir_all(&apps)?;
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=Network Trace\n\
             Comment=Continuous traceroute monitoring with reports for your ISP\n\
             Exec=\"{}\" %U\nIcon={ICON_NAME}\nCategories=Network;Utility;\n\
             StartupWMClass={APP_ID}\nTerminal=false\n",
            appimage.display()
        );
        let old = apps.join(OLD_ENTRY);
        if std::fs::read_to_string(&old).is_ok_and(|t| t.contains("Name=Network Trace")) {
            let _ = std::fs::remove_file(&old);
        }
        let path = apps.join(format!("{APP_ID}.desktop"));
        // Rewrite only when it changed (e.g. the AppImage moved).
        if std::fs::read_to_string(&path).ok().as_deref() != Some(entry.as_str()) {
            std::fs::write(&path, entry)?;
        }
        Ok(())
    }
}
