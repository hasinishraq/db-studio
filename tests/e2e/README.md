# End-to-end drivers

These drive the real app through WebView2's debug port, on Windows, against the test PostgreSQL. They
need the `ws` package and a build with embedded assets.

```bash
npm run build
cargo build -p studio-app --features tauri/custom-protocol
```

Start `target/debug/studio-app.exe` from `src-tauri/` with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`, then run a driver, passing a
folder for screenshots:

```bash
node tests/e2e/core-flows.mjs ./shots
node tests/e2e/ai-and-tables.mjs ./shots
```

`ai-and-tables.mjs` needs the profile `core-flows.mjs` creates, and starts its own stand-in model, so
no real AI provider is involved.

Set `STUDIO_DATA_DIR` to a scratch folder so the run leaves your own connections alone. It works in
debug builds only. The test connection's password still goes to the OS keychain, as
`profile:<id>:password`, so remove that entry afterwards.

