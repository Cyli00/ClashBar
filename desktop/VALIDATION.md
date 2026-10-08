# Validation and release checks

The GitHub Actions `Windows Desktop` workflow has separate frontend and Windows gates:

- Node unit tests, TypeScript/production build and Playwright interaction tests on an Ubuntu runner.
- Rust formatting, domain/process tests and Windows-target Clippy.
- A WinINet integration test in the disposable runner account: save real current-user settings, enable twice, restore, and compare with the original. It is deliberately ignored by normal `cargo test` and requires both an explicit test filter and `CLASHBAR_PROXY_INTEGRATION_TEST=1`.
- NSIS installer build and artifact upload.

Browser tests mock the Tauri IPC boundary. They test frontend behavior, not WebView2, the actual Windows proxy APIs or end-to-end network tunneling. Rust process tests use a controlled fake core to exercise the lifecycle. They do not establish compatibility with every mihomo subscription.

## Windows interactive release check

Before publishing a signed stable release, use a disposable Windows account or VM with a trusted mihomo executable and a valid test profile:

1. Install the NSIS artifact, start the app and select the core. Import YAML with a provider/group name containing Chinese, spaces and `/`. Verify mode selection, node selection and latency result.
2. Confirm an invalid profile or occupied port reports a failure without leaving a child process or changing system proxy settings.
3. Start and stop repeatedly. Enable proxy, close the window, reopen from tray, then quit. Confirm the previous Windows manual/PAC/auto-detect settings and that the owned core has stopped.
4. Enable proxy, change the OS proxy using another client, then stop ClashBar. Confirm the newer settings remain and subsequent enable/disable restores that newer baseline.
5. Kill only the mihomo child. Confirm running state changes and proxy recovery occurs. Force-terminate ClashBar, relaunch it and confirm journal recovery.
6. Verify a real HTTP/HTTPS request uses the selected node; inspect and close that connection. Verify provider update and rule filtering with the test profile.
7. Check tray, duplicate launch, keyboard operation, dark/light appearance and high-DPI WebView2. Do not equate installer compilation with these interactive checks passing.

Never run the opt-in proxy integration test in a user's normal account with other proxy clients active.
