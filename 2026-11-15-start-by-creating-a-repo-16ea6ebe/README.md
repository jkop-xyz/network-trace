# network-trace

A desktop app for Linux and Windows that keeps tracing the route to a host, recording latency and packet loss at every hop over time. When something goes wrong, you can export the evidence as a Report (HTML or PDF) and a CSV to send to your ISP.

See [GLOSSARY.md](GLOSSARY.md) for the terms used throughout (Session, Target, Hop, Sample, Gap, Time Window…).

## Using it

- Type a hostname or IP address and press **Start**. Each Target gets its own Session, and several can run at once.
- The hop table shows loss, latency and jitter per Hop for the chosen **Time Window**. Click a Hop to graph it.
- **Export HTML / PDF** makes a Report for the current Time Window. For PDF, pick "Save as PDF" (Windows) or "Print to File" (Linux) in the print dialog.
- **Export CSV** writes one row per Probe: `timestamp,hop,address,rtt_ms,lost`.
- Closing the window keeps Sessions running in the tray. Use **Quit** in the tray menu to stop them.
- Targets given on the command line start straight away: `network-trace 1.1.1.1 192.168.1.1`.

No admin rights are needed. On Linux this relies on unprivileged ICMP, which most distros allow. If yours doesn't, the app tells you, and this fixes it:

```sh
sudo sysctl -w net.ipv4.ping_group_range="0 2147483647"
```

Downloads are unsigned, so Windows SmartScreen warns the first time you run the `.exe`.

## Development

Needs Rust (stable), Node 22+ and, on Linux, the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (WebKitGTK 4.1, libayatana-appindicator).

```sh
npm install
npm run tauri dev                  # run the app
cargo test -p trace-core           # core tests (probing, storage, stats)
npm run check                      # type-check the UI
cargo run -p trace-core --example synth -- <sessions dir>   # synthetic Session with loss, a Gap and a Route Change
```

Layout:

- `crates/trace-core`: probing (Linux ping sockets, Windows `IcmpSendEcho2`), the Session engine, SQLite storage (one file per Session), stats and CSV export. No UI code.
- `src-tauri`: the desktop shell: commands, tray and printing.
- `src`: the Svelte UI. `src/lib/chart.ts` draws graphs for both the screen and Reports, and `src/lib/report.ts` builds Reports from a list of sections.

Pushing a `v*` tag builds the Linux AppImage and the Windows portable `.exe` and attaches them to a GitHub Release (a pre-release for tags like `v0.1.0-beta.1`).
