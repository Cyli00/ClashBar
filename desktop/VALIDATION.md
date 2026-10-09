# Validation and release checks

The GitHub Actions `Windows Desktop` workflow has separate frontend and Windows gates:

- Node unit tests, TypeScript/production build and Playwright interaction tests on an Ubuntu runner.
- Rust formatting, domain/process tests and Windows-target Clippy.
- A WinINet integration test in the disposable runner account: save real current-user settings, enable twice, restore, and compare with the original. It is deliberately ignored by normal `cargo test` and requires both an explicit test filter and `CLASHBAR_PROXY_INTEGRATION_TEST=1`.
- NSIS installer build and artifact upload.
- A native popup smoke launches the built executable in the disposable Windows runner: hidden startup, no title/resize frame, no taskbar/Alt-Tab window, 360 logical pixel sizing, work-area bounds, second-instance activation and close-to-hide. This does not simulate tray clicks or replace a multi-monitor manual check.

Browser tests mock the Tauri IPC boundary. They test frontend behavior, not WebView2, the actual Windows proxy APIs or end-to-end network tunneling. Rust process tests use a controlled fake core to exercise the lifecycle. They do not establish compatibility with every mihomo subscription.

## Windows interactive release check

Before publishing a signed stable release, use a disposable Windows account or VM with a trusted mihomo executable and a valid test profile:

1. Install the NSIS artifact, start the app, open it from the notification-area icon and select the core. Import YAML with a provider/group name containing Chinese, spaces and `/`. Verify mode selection, node selection and latency result.
2. Confirm an invalid profile or occupied port reports a failure without leaving a child process or changing system proxy settings.
3. Start and stop repeatedly. Enable proxy, close the window, reopen from tray, then quit. Confirm the previous Windows manual/PAC/auto-detect settings and that the owned core has stopped.
4. Enable proxy, change the OS proxy using another client, then stop ClashBar. Confirm the newer settings remain and subsequent enable/disable restores that newer baseline.
5. Kill only the mihomo child. Confirm running state changes and proxy recovery occurs. Force-terminate ClashBar, relaunch it and confirm journal recovery.
6. Verify a real HTTP/HTTPS request uses the selected node; inspect and close that connection. Verify provider update and rule filtering with the test profile.
7. Check left-click open/close, outside click dismissal, pinned state, Escape, native file pickers, and focus moving between the main popup and its separate submenu. Menus should open on hover, support keyboard selection, and close after leaving their trigger/menu bridge. Test the Windows tray overflow as well as the visible taskbar.
8. Test monitors at 100%, 150% and 200% DPI, with different monitor origins and taskbar edges. Resize by switching tabs or collapsing providers; the popup and side menu must remain inside the selected work area. Check dark/light appearance and real WebView2 rendering.
9. Import two profiles, switch while running with system proxy enabled, and try a candidate that passes YAML parsing but fails mihomo validation or startup. Confirm the prior core remains or is restored, the prior profile remains selected on relaunch, and subscription credentials never enter the displayed errors.
10. Compare populated light/dark node panels, group menus, rules, connections, logs and settings against the original screenshots and source listed in `PARITY.md`. Do not equate installer compilation or mocked browser screenshots with complete feature parity.

Never run the opt-in proxy integration test in a user's normal account with other proxy clients active.
