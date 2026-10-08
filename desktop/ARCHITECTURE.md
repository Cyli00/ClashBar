# Windows migration decision

## Decision

Use Rust + Tauri 2 for the new Windows desktop client, with a small TypeScript view layer and the existing upstream mihomo executable as the proxy engine. Do not translate the proxy engine to Rust. Retain the Swift/macOS implementation during migration so feature behavior and upstream fixes remain inspectable.

The current product is a tray utility for configuration, proxy selection, rules, connections and system proxy management. Its HTTP controller boundary is reusable; AppKit, SwiftUI, XPC, launchd and the privileged macOS helper are not portable.

| Option | Fit for this fork |
| --- | --- |
| Rust + Tauri 2 | Shared UI and domain layer with isolated Windows integration; typed IPC, WebView2, tray and installer support. Chosen for future cross-platform evolution. |
| C# + WinUI 3 / WPF | Strong alternative for a permanently Windows-only product and an experienced .NET team. Would still require rewriting the Swift UI and services. |
| C++ + Qt | Viable cross-platform native toolkit, but this repository has no reusable C++ implementation to offset its integration and maintenance cost. |

This is a project-specific tradeoff, not a performance benchmark or a claim that Tauri preserves the original macOS application's package size or memory usage. Windows requires WebView2.

## Boundaries

- TypeScript owns presentation and interaction state. Rust owns filesystem access, secrets, configuration derivation, network calls, process lifecycle and proxy changes.
- IPC exposes named business operations; it does not expose a generic HTTP client, filesystem API or shell command to the WebView.
- The web surface contains only bundled assets, with restrictive CSP and no remote content execution. Untrusted node names, rules, logs and errors are rendered as text.
- A managed local controller uses loopback and an ephemeral secret. Control requests bypass system proxies and do not follow redirects. Subscription transport is separate and bounded.
- All lifecycle/proxy mutations are serialized. Readiness, validation and controller failures remain visible; a spawned process alone is not proof the client is connected.
- User proxy settings are a reversible transaction: durable snapshot, apply, ownership check, restore. An unrelated proxy owner must not be overwritten.
- The client owns only its child process, never all processes named mihomo. Windows Job Object lifetime binds cleanup to the application where supported.

## Incremental migration

The initial PR covers the usable local proxy loop and installs a Windows build pipeline. Follow-up work should be individually reviewable: remote-machine parity; streaming metrics/logs; a separately designed privileged Windows TUN service; SSID policy; startup/update lifecycle; localization; macOS/Linux platform adapters.

The tray popup follows the original SwiftUI layout tokens: 360 logical pixels wide, 8-pixel inner margins, compact monospaced controls and a fixed header/footer. The main window starts hidden, has no decorations or taskbar entry, and anchors to the tray monitor's work area. A separate owned WebView displays adjacent menus rather than expanding the main window into an invisible click-catching surface. Popup pinning, focus transitions, native file pickers and tray clicks share one native state owner.

Profiles use content-addressed source files and one atomic settings pointer. Importing retains prior profiles. A live switch validates the candidate with the selected core before stopping the working one; startup and proxy restoration precede committing the new pointer. Failed activation attempts restore the previous managed process. An invalid legacy profile does not prevent opening the UI to re-import.

Do not remove the Swift implementation until replacement functionality and platform tests establish parity. See [README](README.md) for the explicit current support matrix and limitations.

## References

- Existing domain contracts: `Sources/ClashBar/Services/MihomoAPIService.swift`, `CoreService.swift`, `SystemProxyService.swift`.
- [Tauri architecture](https://v2.tauri.app/concept/architecture/), [system tray](https://v2.tauri.app/learn/system-tray/), [capabilities](https://v2.tauri.app/security/capabilities/).
- [Windows internet options](https://learn.microsoft.com/en-us/windows/win32/wininet/setting-and-retrieving-internet-options), [Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
- [Microsoft WinUI 3](https://learn.microsoft.com/en-us/windows/apps/winui/), [Qt system tray](https://doc.qt.io/qt-6/qsystemtrayicon.html).
