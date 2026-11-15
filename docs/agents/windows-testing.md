# Testing network-trace on Windows

Instructions for an agent (or person) on a Windows 10/11 machine. The app is developed on Linux; CI compiles it and runs the core tests on a Windows runner, but nobody has yet checked the Windows build against a real network or a real desktop. That is your job.

Read `GLOSSARY.md` first and use its terms (Session, Target, Hop, Probe, Sample, Gap, Time Window, Report, Raw Export) in what you report.

## Ground rules

- **Test; don't change the code** unless the user asks you to. If you do fix something, work on a branch and open a PR. Never push to `main`.
- **Report findings as GitHub issues** in `jkop-xyz/network-trace` (see `docs/agents/issue-tracker.md`), one issue per bug, labelled `needs-triage`. Finish with one summary issue titled `Windows test report: <version>` that links the others and lists what passed.
- Many checks below need someone to look at the screen. If you can't see or control the desktop, ask the user to do those steps and tell you what they saw, or attach screenshots.
- Each issue should give the Windows version (`winver`, or `[System.Environment]::OSVersion`), the release tag or commit, steps, expected vs. actual, and any screenshots or error text.

## 1. Get the build

Test the latest pre-release unless the user names another:

```powershell
gh release list -R jkop-xyz/network-trace --limit 3
gh release download <tag> -R jkop-xyz/network-trace -p "*.exe" -D $env:USERPROFILE\nt-test
```

The `.exe` is portable: no installer. It is unsigned, so SmartScreen warns on first run ("More info" → "Run anyway"). Record whether that happens; it's expected, not a bug. WebView2 ships with Windows 11 and current Windows 10; if the app shows a blank window or fails to start, check that WebView2 is installed.

To build from source instead (only needed for sections 2 and 3), you need [rustup](https://rustup.rs) (MSVC toolchain), the Visual Studio C++ Build Tools, and Node 22+:

```powershell
git clone https://github.com/jkop-xyz/network-trace; cd network-trace
npm ci
npx tauri build --no-bundle      # → target\release\network-trace.exe
```

## 2. Core tests (needs the Rust toolchain)

```powershell
cargo test -p trace-core
```

All tests should pass. `probe::tests::*` sends real ICMP to 127.0.0.1 and ::1.

## 3. The Windows probing code on a real route (most important)

`crates/trace-core/src/probe/windows.rs` calls `IcmpSendEcho2` / `Icmp6SendEcho2` through hand-written FFI. Up to now it has only been run against loopback. Compare it with Windows' own `tracert`:

```powershell
cargo run -q -p trace-core --example hops -- 1.1.1.1
tracert -d -w 2000 1.1.1.1
```

Expected:
- Each line of `hops` is `TimeExceeded { from: <router>, rtt }` until the last, which is `Echo { from: 1.1.1.1, rtt }`.
- The router addresses match `tracert` hop for hop. Hops shown as `*` in `tracert` may be `Timeout` here.
- Round-trip times are in the same range as `tracert` (the app uses a µs timer, `tracert` rounds to ms).

Also try a second Target (e.g. `8.8.8.8`) and, if the machine has IPv6 (`ping -6 2606:4700:4700::1111` works), `cargo run -q -p trace-core --example hops -- 2606:4700:4700::1111`. Wrong or unspecified addresses (`0.0.0.0`, `::`), every hop timing out, or `Unreachable` where `tracert` shows a normal hop are bugs: include the full output of both commands.

## 4. The app

Start it from PowerShell so you can pass Targets (each starts a Session immediately):

```powershell
& $env:USERPROFILE\nt-test\network-trace-windows-x64.exe 1.1.1.1 8.8.8.8
```

Data lives in `%APPDATA%\app.networktrace.desktop\` (`sessions\*.sqlite`, one file per Session; `settings.json`). Each Session file has tables `meta`, `samples(ts, interval_ms, target_known)` and `probes(sample_id, hop, address, rtt_us)`. Lost Probes have a NULL `rtt_us`. You can inspect them with Python's `sqlite3` to confirm what the UI shows.

Check each item and note pass/fail:

**Monitoring**
1. Both Sessions appear in the sidebar with a green dot; the hop table fills within a few seconds; the last row has a "Target" badge.
2. Hop addresses match `tracert`; reverse DNS names appear under some addresses after a short while.
3. The Target graph draws a line; the legend says "Times are UTC±hh:mm" with the right offset.
4. Change "Sample every" to 1 s and back. No "no data" band should appear from the change.
5. Click a Hop number: its graph appears and the page scrolls to it.

**Loss and Gaps (the app's whole purpose)**
6. With a Session running, disconnect the network (airplane mode or disable the adapter) for about 60 s, then reconnect. Expected: that minute shows as **loss** (red bars, 100% at the Target), *not* as "no data". The Target's loss % rises. The status dot goes red then green again.
7. Put the PC to sleep for a few minutes with a Session running, then wake it. Expected: the sleep shows as a grey **"no data"** band, not loss.

**Stop, Resume, Delete**
8. Stop a Session: it shows "Stopped" with **Resume** and **Delete**. Resume: the *same* entry continues (no second entry), and the stopped period shows as "no data".
9. Delete a stopped Session: it disappears, and its `.sqlite` file is removed. A delete failing with a "file in use" error is a known risk on Windows: report it if you see it.

**Tray and single instance**
10. Close the window: a one-time "Still running" notice appears; the tray icon stays; hovering it shows "N Sessions running". Clicking it brings the window back.
11. While it's in the tray, launch the `.exe` again: the existing window comes to the front, and no second copy starts (Task Manager shows one `network-trace` process tree). Launching with a new Target (`... 9.9.9.9`) adds that Session to the running copy.
12. Tray menu → "Quit": the app exits; reopening shows the Sessions as stopped.

**Exports**
13. **Export Report (PDF)**: a Report window and the print preview open. Save as PDF and open the PDF:
    - The summary sentence, graphs, and red rows for carried loss are visible. Background colours must print; check the red table rows and the yellow/blue boxes.
    - **Headers/footers must not show a file path or the Windows user name.** A URL like `http://ntreport.localhost/report.html` is acceptable but worth noting.
    - The yellow "To save this Report as a PDF…" banner must *not* appear in the PDF.
14. **HTML**: the save dialog offers `.html`; the saved file opens in a browser and looks like the PDF. "Show in folder" opens Explorer with the file selected.
15. **Raw Export (CSV)**: opens fine in Excel; the first column is ISO 8601 with your UTC offset; lost Probes have an empty `rtt_ms` and `lost` = 1.

**Other**
16. Right-click a public hop address → "Who owns this address?" shows the organisation and address block. A private address (192.168.x.x) says it isn't publicly registered.
17. Right-clicking elsewhere shows no browser menu (Back/Reload); right-click in the Note box still offers copy/paste.
18. **Overview**: switch between 1×1, 2×2 and 3×3; each cell shows figures and a graph that fits the cell.
19. **Storage** shows sizes that roughly match the `.sqlite` files on disk; "Open folder" opens Explorer there.
20. Resize the window very large and small: no overlapping or cut-off controls.

## Known risk areas (look harder here)

- `probe/windows.rs`: status-code handling (`IP_TTL_EXPIRED_TRANSIT` 11013, `IP_REQ_TIMED_OUT` 11010, unreachable codes 11002–11005/11040) and the IPv6 reply layout.
- WebView2 printing: headers/footers and background colours.
- Tray icon and the single-instance hand-off.
- File locking when deleting a Session that the UI is reading.
