# Tauri as the desktop shell

The app must be a native desktop app on Linux and Windows, show dense live graphs, and produce an HTML Report that matches what is on screen. We chose Tauri: Rust does the probing and storage, and the interface is HTML rendered by the operating system's web engine (WebView2 on Windows, WebKitGTK on Linux). The main reason is that the graph and hop-table code used on screen also produces the exported Report, so the two can never drift apart.

## Considered Options

- **egui / iced (pure Rust, drawn natively):** no web engine at all, but the Report would need a second rendering path for the same graphs.
- **Qt:** the most traditional native look, but heavy, with harder cross-compilation and licensing to consider.
- **Wails (Go + webview):** the same model as Tauri; Rust was preferred for unprivileged ICMP handling on both platforms.

## Consequences

- Linux rendering depends on WebKitGTK, whose quirks (printing included) differ from WebView2, so both engines must be tested.
- The Linux AppImage bundles WebKitGTK and is noticeably larger than the Windows binary.
