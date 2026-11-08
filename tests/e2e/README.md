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

## schema-and-databases.mjs

Covers connecting without a database, the bold default, row export, the resizable log, and schema
export, preview and apply. It needs PostgreSQL on port 54329 and MySQL on port 33069. Set `E2E_APP_PID`
to the app's process id, because `file-dialog.ps1` answers that process's native file dialogs through
their window handles and sends no keystrokes. Windows only.

If an installed copy is open, set `WEBVIEW2_USER_DATA_FOLDER` to a scratch folder. Two copies with
different launch flags cannot share a WebView2 profile, and the second fails with 0x8007139F.

## grid-and-diagram.mjs

Covers inline grid editing (edit, Tab to the next cell, new rows, marked deletes, the pending bar,
the unsaved-changes prompts, a save refused because the row changed elsewhere, the form for tables
without a key) and saving the diagram as PNG, JPEG and PDF through the real save dialog, and setting a
remembered default schema from the right-click menu, which must survive a reconnect and show in
the assistant. Needs
PostgreSQL on port 54329 and `E2E_APP_PID`, as above. Windows only.
