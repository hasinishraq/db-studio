# Progress

The user asked for the whole application in one go, so phases run back to back in one session
instead of one session each with a plan-approval stop. Phase order, gates and "never commit" still
apply. Nothing has been committed; the human reviews and commits.

## Environment (Windows 11, this machine)
- VS 2022 Build Tools (C++ workload) and Windows SDK 10.0.26100 were installed for this project.
- Docker is not installed. Test databases are portable zips in a scratch folder, bound to 127.0.0.1:
  PostgreSQL 17.6 on 54329 and 54330 (TLS), MySQL 8.4.6 on 33069, MariaDB 11.4 on 33079.
- The machine already runs `MySQL80` and `postgresql-x64-18` services (3306/33060, 5432). They are
  the owner's and must not be touched; the test servers avoid their ports (`--mysqlx=OFF`).
- `npx tauri` stands in for `cargo tauri` (the CLI comes from npm, which avoids a long compile).

## Phase 0 - Bootstrap
- Built: workspace, all crates, Tauri 2 app, lint settings, compose file, CI and release workflows.
- Deviations: scaffolded by hand instead of `npm create tauri-app`. `docker/tls/gen.sh` sets
  `MSYS_NO_PATHCONV` so it works under Git Bash.
- Versions differ from the guide where the real crates moved: sqlx 0.9, rusqlite 0.39 (newest that
  shares libsqlite3-sys with sqlx), mysql_async 0.37, tokio-postgres-rustls 0.14, sqlparser 0.63,
  russh 0.65; specta pinned to `=2.0.0-rc.26` and specta-typescript `=0.0.13` as tauri-specta requires.
- Decision: `ring` crypto everywhere. `aws-lc-rs` needs NASM on Windows (guide section 11 allows this).

## Phase 1 - studio-types
- Built: all DTOs, ids, `Value`, `AppError`. 8 tests.
- Deviations: added `RunEvent::Columns` (see Phase 3), `ErrorCode::{HostKeyUnknown, DirtyTransaction}`,
  and DTOs for audit, drafts, history, masking and planned SQL.
- The 11 filter operators are my own choice; the Electron source was not available.

## Phase 2 - studio-store
- Built: migrations, repositories, keyring worker thread, memory store, hash-chained audit log.
  16 tests, including detection of a tampered and of a deleted row.
- Deviation: `Store::open(dir, secrets)` takes the secret store so `has_password` can be filled.

## Phase 3 - studio-db
- Built: traits, `FilterBuilder`, conformance kit. 10 unit tests.
- Deviations from the architecture document, both deliberate:
  - `RowSink` carries `RowEvent::{Columns, Batch}`; the grid needs columns before the first row.
  - `Session::canceller()` became `Session::acquire(scope) -> Conn`, and `Conn` owns the canceller.
    On a pool a session-wide canceller cannot know which connection a run uses, and a script must
    stay on one connection. `ConnScope::Isolated` keeps AI reads out of a manual transaction.
- Decision: LIKE escape is `!`, which behaves the same on every engine and `sql_mode`.

## Phase 4 - driver-postgres
- Built: all four TLS modes, streaming, cancel, read-only guard, metadata, paging, EXPLAIN.
  15 tests against real PostgreSQL 17.6, including the conformance kit and every TLS mode.
- Deviation: results use the simple (text) protocol instead of binary plus a custom `FromSql`. That
  gives the server's own text for dates by construction and covers every type. `prepare` still runs
  first: it yields column types, and the server refuses to prepare two commands.
- Decision: pooled connections recycle with `ROLLBACK; RESET ALL`.

## Phase 5 - driver-mysql
- Built: TLS options, streaming, KILL QUERY cancel, watchdog, metadata, paging, EXPLAIN.
  10 tests against real MySQL 8.4.6.
- Decision: `prep` runs first because mysql_async allows multi-statements by default.
- Decision: a client watchdog backs up `MAX_EXECUTION_TIME`, which only covers SELECT.
- TLS is tested against a real server when `TEST_MYSQL_TLS_URL` is set (the compose `mysql` service
  has no certificates, so CI skips it unless that changes).
- Gap: `verify-ca` is as strict as `verify-full` on MySQL. mysql_async forgives a name mismatch by
  searching the error text for `NotValidForName`, and current rustls words it differently. This fails
  safe, and a test pins it. Server certificate details are reported for PostgreSQL only.

## Phase 6 - studio-safety
- Built: lexer, splitter, allow-list classifier, deny-lists, risk, approvals. 165+ rows per dialect.
- Bug found by the tests: a long `1 + 1 + ...` chain overflowed the stack and aborted the process.
  Fixed with a comma-free token cap (fails closed) and a dedicated 64 MB thread that also turns a
  parser panic into a fail-closed verdict. Regression rows added.
- Deviation: splitting is lexical and each piece must parse to exactly one statement, because AST
  spans are not reliable byte offsets. Any lexer and parser disagreement fails closed.
- Table rows changed after they failed, each checked: `SELECT FROM WHERE` is valid on PostgreSQL
  (it is a read of a table named WHERE, which the server rejects), so it is `Auto` there and stays
  `Write` elsewhere. `FOR NO KEY UPDATE` is `Write` because the parser fails closed on it.
- Gap: the case tables are written fresh, not ported, because `readonly.test.mjs` and
  `classifier.test.mjs` were not available.

## Phase 7 - studio-session
- Built: connect, prepare, `run_user`, `run_ai_read`, `run_ai_approved`, cancel, manual transactions.
  27 tests on a fake driver (behind the `testing` feature, shared with the app and AI crates).
- Deviations: `run_ai_*` return collected rows instead of taking a sink. Tokens are also bound to
  their door: an AI token fails in `run_user`, an editor token fails in `run_ai_approved`. Typed
  `COMMIT`/`ROLLBACK` is refused in manual mode so state cannot drift.

## Phase 8 - studio-app
- Built: state, setup, commands, bindings export, capabilities, CSP. 16 tests, including the Rule 6
  scan proving only `approve_query` mints tokens.
- Deviation: `build.rs` links the Windows manifest itself; Tauri embeds it in the app binary only,
  so test binaries otherwise die with STATUS_ENTRYPOINT_NOT_FOUND.
- Decision: bindings use `dangerously_cast_bigints_to_number`. Only counts and millisecond
  timestamps are 64-bit over IPC; database 64-bit values are strings in `Value::BigInt`.
- Gap: Tauri `Channel::send` does not wait for the UI, so backpressure ends at the Rust side of IPC.

## Phase 2-8 addenda found during later phases
- `driver-mysql`: a plain `START TRANSACTION READ ONLY` does NOT stop DDL on MySQL or MariaDB. DDL's
  implicit commit ends the transaction and the statement runs. Confirmed on both servers: a table was
  really dropped. The safety suite found this. The driver now also sets `SET SESSION TRANSACTION READ
  ONLY`, and `ddl_cannot_escape_a_read_only_transaction` covers it. This corrects the guide's Phase 5
  task 5 and the architecture's Table 3.1.
- `driver-mysql`: MariaDB's `max_statement_time` interrupts `SLEEP` without an error, so a timed-out
  statement looked successful. Its timer can also fire a few ms early, so a statement ending within
  50 ms of the limit counts as stopped by it (MariaDB only). Stable over 25 runs.

## Phase 9 - Frontend shell, connections, explorer
- Built from scratch; the Electron source was not available, so nothing was ported.
- Decisions: colour means risk only (safe, change, danger). A band down the window edge shows the
  environment. Atkinson Hyperlegible for UI, JetBrains Mono for SQL and cells.
- Deviation: one `src/api/index.ts` rather than a file per domain. `SSH` tab is real, not a placeholder,
  because Phase 14 is done.

## Phase 10 - Editor, run flow, streaming
- Built: Monaco with schema completion, `runFlow`, `ResultStore`, virtual grid, history, autosaved drafts.
- Bug found only by the end-to-end run: `withRows` was memoized, so the grid appeared only after a run
  ended. Now subscribed to the store.
- Measured in the real app: 1,000,000 rows in about 7 s, worst UI stall 19-24 ms, about 37 rows in the DOM.

## Phase 11 - Table data, structure, ER
- Built: `plan_row_change`, `plan_create_table` (snapshot tests per dialect), table tab, structure view,
  ER diagram, row, filter and create-table dialogs.
- Gap: the ER diagram draws at most 60 tables, in a simple grid rather than a routed layout.

## Phase 12 - studio-ai
- Built: `Provider` trait, rig adapter, fake provider, loop, review gate, memory, offline engine, hub.
  43 tests, 5 of them driving the real rig path against a local server speaking the OpenAI format.
- Addition: memory scrubs shared values from the assistant's text. Without it, a model quoting a shared
  row would put that row into memory, which rule 9 forbids.
- Deviations: masking matches by column name across tables, because result columns do not name their
  table; it over-masks, which is the safe direction. `run_ai_approved` takes exactly one statement.
- Gap: agent test cases are written fresh, not ported from `agent.test.mjs`. Not tested against a real
  Ollama or OpenAI service; neither is available here.
- Gap: `ollama` uses Ollama's OpenAI-compatible route. Its tools rejection is matched by message text.

## Phase 13 - AI frontend
- Built: chat tab, review cards, charts, conversations, AI settings, masking editor.
- Deviation: no separate side panel; the assistant is one tab. A single-series chart has its own
  `--series` colour, kept apart from the risk colours.

## Phase 14 - studio-tunnel
- Built: tunnels with password, key and agent auth, host key pinning, trust flow, SSH tab, certificates.
- Tested with a real in-process russh server instead of the OpenSSH container: forwarding, both auth
  kinds, unknown and changed keys, and that no credential is sent to an unverified server.
- Real PostgreSQL through a real SSH handshake is tested through `TunnelConnector`, including
  verify-full TLS end to end, and a host name the certificate does not cover being refused.
- Bug found that way: a peer that accepts and goes silent left `connect` waiting forever, because
  `connect_timeout` covers only TCP. The PostgreSQL handshake now has one overall deadline.
- Gaps: agent auth compiles but was never exercised. Host certificates are refused.

## Phase 15 - Production features
- 15a-15g built. Export dialogs open in Rust, so the webview never supplies a path. The import file is
  held behind an opaque id, and the approval covers a hash of the file, so editing the file after
  review fails the approval.
- 15g masking applies in Rust, so hidden values never reach the webview.
- Gaps: export writes binary columns as their 64-byte preview, not the full value; Excel stops at the
  sheet's row limit. `file_hash_and_rows` reads the whole CSV into memory once.
- Scale: a 100k-row import, then one bad row rolling back 5,000 more, passes on PostgreSQL, MySQL,
  MariaDB and SQLite. 1M-row exports pass to CSV, JSON lines and SQL on PostgreSQL; Excel was run at
  200k rows. Flat memory during export was not measured.
- Not built: optional 15h (encrypted app database). The `encrypt_store` setting exists but does nothing.
  Backup and restore through `pg_dump` (architecture table 11.1 only) is also not built.

## Phase 16 - Release
- Built: legacy importer, updater plugin and capability, crash file, diagnostics, release workflow.
- Gaps, each needing the owner's accounts or data:
  - Updater: no signing key or endpoint, so nothing calls `check()` yet. See `docs/RELEASING.md`.
  - Signing: no Apple or Windows certificates. Installers are unsigned.
  - Legacy import: the JSON shape is inferred from field names, because no real export was available.
    It has only been tested against a sample I wrote. `ai-memory.json` is not imported.
  - macOS and Linux builds were never run; CI will be the first time.
- Security note: `approve_query` trusts the webview's call. A compromised page could call it directly,
  so the human click is enforced by the UI, not by Rust. A native confirmation dialog would close this.

## Dependency audit
- `cargo deny check` passes with six documented exceptions in `deny.toml`. One is a real advisory:
  RUSTSEC-2023-0071 in `rsa`, reached through `russh` for RSA SSH keys, with no fixed release yet.
  The others are unmaintained crates under specta, Tauri's GTK stack and the opener plugin.
- `rustls-pemfile`, the one flagged crate that was a direct dependency, was removed.
- `cargo audit` was not run separately; `cargo deny` reads the same advisory database.

## After hand-over: text size and run log
- Ctrl + and Ctrl - (with or without Shift, so Ctrl _ too), Ctrl 0 to reset, the command palette and
  Settings all change the size. It uses the webview's own zoom, because CSS zoom breaks Monaco's mouse
  positions. This needs the `core:webview:allow-set-webview-zoom` permission, which is new. The level is
  kept in localStorage.
- SQL tabs and table tabs show a log under the data, one line per statement. It lasts as long as the tab.
- A SQL tab keeps the last rows on screen after a statement that returns none, and says they are from
  before the change. Refresh repeats only the statements Rust called read-only. Nothing re-runs by itself.
- Fixed: the review dialog cut statements by byte position out of a JavaScript string, so text after
  non-English characters showed the wrong slice of SQL. `spanText` in `src/lib/sqlText.ts` now does this.
- E2E note: an installed copy that is open holds the shared WebView2 profile, and a second copy with a
  debugging port then fails to start with 0x8007139F. Set `WEBVIEW2_USER_DATA_FOLDER` for test runs.
- Checked: 250 Rust tests, 42 frontend tests, clippy and fmt clean, E2E 39/39 and 20/20.

## After hand-over: rows, databases and schema files
- The log under the data is a resizable panel in SQL, table and diagram tabs.
- Query results have row selection (click, Ctrl, Shift, Ctrl+A, Esc). Export writes the selected rows,
  or the filtered rows, or else the whole result. `export_rows` writes what the grid holds, so masked
  values stay masked, and it refuses SQL inserts when a binary value was cut short in the grid.
- A table tab's Export honours selected rows and filters. Before, it wrote the whole table. Filters are
  written as literals (`FilterBuilder::inline`) because the export streams one statement; that SQL
  still goes through the classifier as a read. A table tab's selection now clears with the page.
- The database field is optional on MySQL and PostgreSQL. `use_database` chooses the default, shown
  bold, and refuses unknown names, open transactions and, on PostgreSQL, a running query. MySQL points
  each pooled connection at it when taken. PostgreSQL opens a new session and only then closes the old
  one, and its table and diagram tabs close because they name objects in the old database. A blank
  PostgreSQL database opens `postgres`. The choice lasts for the session and is not saved in the profile.
- The diagram tab holds schema export and import. Export writes tables in dependency order, then
  PostgreSQL foreign keys, then views. `ForeignKeyInfo.definition` is new because the rebuilt PostgreSQL
  DDL has no foreign keys. Not exported: types, triggers, functions. A PostgreSQL column of a custom
  type therefore fails on import. MySQL foreign key cycles cannot be ordered and need
  `SET FOREIGN_KEY_CHECKS=0` by hand.
- Import reads a file of at most 1 MB (the SQL limit) in Rust, draws it with
  `studio_safety::schema_outline` without running it, and applies it only through the normal review.
  A failed statement leaves earlier ones applied, and the review says so.
- The diagram has no table or column limit, with pan, zoom, drag and column-to-column lines. Positions
  are not saved.
- `tests/e2e/schema-and-databases.mjs` drives real file dialogs with `file-dialog.ps1`, on Windows only.
- Checked: 275 Rust tests with every server up, 48 frontend tests, clippy, fmt and `cargo deny` clean,
  E2E 39/39, 20/20 and 43/43. The PostgreSQL and SQLite exports were rebuilt from their own scripts.
  MySQL was tested for ordering only, not rebuilt.
- Open: `conformance` failed once at `execute.rs:59` (the 10,000-row read) during a full workspace run,
  then passed three full runs and three runs per driver. The message was not captured.

## After hand-over: clearer interface
- Colour still means risk only. Elements are told apart by three depth steps (ground, panel, raised),
  a 3:1 `edge` outline on every control, and one mark for where you are: `.here`, an ink bar on the
  `pick` background. It is used on the active tab, side icon, chosen database, selected rows and the
  current result or view. Focus rings are ink, not green.
- Theme colours accept opacity now (`border-line/40` and the like). Before, those classes produced no
  CSS, so the grid's row dividers never showed.
- Buttons have a `quiet` tone for toolbar extras. Schema row actions are icon buttons with labels for
  screen readers. The table tab's Data/Structure switch is a segmented control and is no longer green.
- `E2E_THEME=light` runs `core-flows.mjs` and `schema-and-databases.mjs` in the light theme.
- Checked: dark and light screenshots of every screen the suites cover. E2E 39/39, 20/20 and 43/43 in
  dark; 39/39 and 43/43 in light.

## After hand-over: new database, left bar
- "New database" in the side panel, for MySQL and PostgreSQL. `plan_create_database` writes the
  statement in Rust with the name quoted; MySQL may add a character set from letters, digits and `_`
  only. It goes through the normal review, and "Use it now" then makes it the default.
- The left bar items share one size, carry a label, and mark the open Assistant, Audit or Settings tab.
- Checked: 278 Rust tests, 48 frontend tests, clippy and fmt clean, E2E 39/39, 20/20 and 49/49.
- Test note: the "your database untouched" check in the local E2E script compares file stamps, so it
  reports False when the installed app is in use during a run. That happened once, from pid 17316.

## After hand-over: collapsing, reordering, closing results, button text
- Connections and PostgreSQL databases collapse and expand. Collapse all and Expand all appear when
  there is more than one connection. A search still shows matches in a collapsed connection, and a
  collapsed one names its database in use. State is kept in localStorage.
- Connections reorder by dragging the grip, or with Alt+Up and Alt+Down on it. The order is kept in
  localStorage (`studio.connectionOrder`), not in the profile. Pointer events are used because Tauri
  takes over HTML drag and drop on Windows for file drops.
- Query results have a Close button. It clears and hides the results and the log until the next run.
- Text on safe, change and danger uses `--on-safe`, `--on-change` and `--on-danger` per theme.
  Dark: dark text, 5.8, 7.1 and 5.3:1. Light: white, 5.2, 4.9 and 5.9:1. Before, light Run was 3.3:1.
- Fixed: a dropped MySQL database stayed the default. `list_schemas` now forgets it, and the panel
  clears the bold mark.
- Checked: Rust and frontend tests, clippy, fmt, E2E 39/39, 20/20 and 61/61 in dark and 61/61 in light.

## After hand-over: schema choice and more settings
- A PostgreSQL schema can be chosen like a database: bold, "in use", and named in the query toolbar.
  `use_schema` checks the name, refuses open transactions, and sets `search_path` to the schema then
  `public` on each pooled connection when taken. The schema in use is read from the server on connect.
- Settings has sections and saves each change (the API key keeps its own button, for the keychain).
  New: editor font size, tab size, wrap, line numbers, overview map, suggestions; statement timeout
  and row limit for queries you run (enforced in Rust; exports and the assistant keep their own);
  NULL text; row height; open a query tab on connect; the Export button's format.
- "Run safe reads without asking" and "Let the assistant see result rows" were settings fields that
  nothing read. They now set the starting value of the matching switches in each new chat. The rules
  in Rust are unchanged, including each connection's assistant policy.
- The editor theme used the old background colours; it matches the workspace again.
- E2E helper `t.label` now ignores hidden labels; it had matched Settings' "Model" for "Mode".
- Checked: Rust and frontend tests, clippy, fmt, E2E 39/39, 20/20 and 74/74.

## After hand-over: one popup style, no side bars
- Coloured bars down the left edge are gone everywhere (review popup, AI review card, host key, TLS
  notes, server report, errors, plan nodes, run log, selection). They replace the ink bar noted above.
- A blue `--accent` (with `--on-accent`) now marks what you act on and where you are: primary
  buttons, focus ring, text selection, the open side panel, and a 2px underline on the open tab.
  `.here` is only the picked background. Safe, change and danger still mean risk only.
- Every popup shares `SURFACE` and `OVERLAY` from `components/ui` (rounded, thin border, `shadow-pop`).
  The review popup is the same as any other; risk shows as an icon by the title (`RiskIcon`) and the
  Run button's colour. Guards and button disabling are unchanged.
- New `Callout` (info, change, danger): full thin border, faint tint, icon. `ErrorNote` uses it.
  Run log entries get a small coloured dot; costly plan nodes a tinted box and the word "Costly".
- The environment band is kept as it was.
- Checked: typecheck, frontend tests 51/51, production build. Not checked by eye in the running app.

## After hand-over: inline grid editing, diagram images
- Table tabs edit cells in place: double-click, Enter or F2; Enter or Tab keeps, Esc drops, Alt+N or
  the NULL button sets NULL. Edits, new rows (top of the grid, "default" until typed) and marked
  deletes stay pending in a bar with Discard and Review and save. Right-click reverts a cell or row.
  Paging, sorting, filtering and closing the tab ask before dropping pending edits. While edits are
  pending the grid keeps the rows they were made on, even if a refresh lands.
- Rust decides what may be edited (`grid_edit_info`): a primary key is required; masked and binary
  columns are locked. Tables without a key, and read-only connections, say why in the grid; tables
  without a key keep the row form.
- `plan_row_changes` writes one statement per row: deletes, then updates, then inserts. An UPDATE
  also matches the loaded value of each edited column, except float, JSON and unknown types, so a
  row changed elsewhere matches nothing. An empty new row is `DEFAULT VALUES` (`() VALUES ()` on
  MySQL). At most 1,000 changes per save.
- New door `SessionManager::run_changes` (command `run_row_changes`): grid tokens only; every
  statement must be INSERT, UPDATE or DELETE and change exactly one row; `BEGIN`/`COMMIT`, or a
  `SAVEPOINT` when a manual transaction is open, and a rollback otherwise. Audit outcomes are `ok`
  or `rolled back`.
- Deviation: `run_user` now refuses grid tokens, so the row form also uses the new door, and
  `plan_row_change` deletes one row per statement instead of one `OR` statement. The audit test that
  used a grid token in `run_user` now uses an editor token; what it checks is unchanged.
- Deviation: MySQL connections set `CLIENT_FOUND_ROWS`, so an UPDATE reports rows matched, as on
  PostgreSQL and SQLite. Without it, saving an unchanged value reports 0 rows. This also changes
  the affected count shown for typed MySQL UPDATEs.
- Masking now also stops `plan_row_change` from writing a masked column or using one as a key.
- The diagram saves as PNG, JPEG (both 2x, white) or PDF (vector, one page sized to the diagram,
  shrunk under 14,000 pt). The webview sends a standalone light SVG (`lib/diagramExport.ts`); Rust
  renders it with resvg and svg2pdf and opens the save dialog. The SVG is capped at 8 MB, may not
  reference any image, file or URL, and rasters are capped at 16,384 px a side and 40 MP.
- Decision: Atkinson Hyperlegible and JetBrains Mono TTFs (OFL, licences beside them) are bundled
  in `src-tauri/fonts/` for the renderer, which cannot read the UI's woff2 files. No system fonts.
- `deny.toml`: two more unmaintained-crate exceptions, `rustybuzz` and `ttf-parser`, under resvg
  and svg2pdf. Not vulnerable, no replacement; they only parse the bundled fonts.
- Checked: 325 Rust tests, 65 frontend tests, clippy, fmt, `cargo deny`, typecheck.
- Not run: the PostgreSQL, MySQL and MariaDB cases of `studio-session/tests/grid_real.rs` (SQLite
  passed) and the new `tests/e2e/grid-and-diagram.mjs`. The portable test servers were gone. Not
  checked by eye in the running app.

## After hand-over: charts in the assistant
- Why it failed before: the model had no way to ask for a chart and the prompt never mentioned
  charts; the app only guessed a single-series bar or line for 2 to 60 rows.
- `run_sql` takes an optional `chart` { type, x, y[], title }: bar, horizontal_bar, stacked_bar,
  line, area, pie, donut, scatter. It names columns only; values always come from the rows the
  database returned, and no row is sent to the model for it (rule 9 unchanged). `studio_ai::chart`
  parses it (at most 5 series, pie one) and checks the columns against the result; a bad request
  never stops the query, and the model is told why no chart was drawn. The prompt says when and
  how to ask. `AiEvent::QueryResult.chart` carries it to the UI.
- Every result has a "Show as" chooser (table or any form); forms that do not fit are greyed out
  with the reason (`buildChart` in `lib/chart.ts`). Pie and donut fold past 6 parts into Other and
  refuse negatives; bars stop at 40 rows, horizontal at 50, scatter at 3 series.
- `ResultChart` draws everything in one SVG (title, legend for 2+ series, ticks, tooltips). "Save
  chart" sends a standalone light SVG to `export_diagram`, so PNG, JPEG and PDF reuse the diagram
  renderer and Rust's save dialog.
- Decision: series colours are the dataviz reference palette's first five hues (`--chart-1..5`,
  plus `--chart-other`), validated for colour vision deficiency in both themes on this app's
  surfaces. The only set avoiding the risk hues (blue, violet, magenta) failed that check, so some
  chart hues sit near safe and change. Charts always carry a legend or labels, so colour never
  means anything alone. Light aqua, yellow and magenta are under 3:1 on the surface; the legend
  and the table view are the required relief.
- Checked: 335 Rust tests, 79 frontend tests, clippy, fmt, `cargo deny`, typecheck. Every form was
  rendered through the export path and looked at. Not tried against a real model or in the app.

## After hand-over: default database from the side panel, assistant scoped to it
- Right-click a database (PostgreSQL databases, MySQL databases) or a PostgreSQL schema, or press
  Shift+F10 or the Menu key on it: Use as default, Use as default and remember, Forget remembered
  default, New query here, and for schemas New table and Diagram. Left click works as before.
- Remembering saves the profile's `database` and, new, `default_schema` (migration 0002), through
  `remember_default`, which reads the profile from the store and checks the names in Rust rather
  than taking a whole profile from the webview. On connect a remembered schema is used if it still
  exists; otherwise the server default stays, and the connection never fails for it. A remembered
  schema is not carried to another PostgreSQL database.
- The assistant works on the database in use only. Before, on MySQL and MariaDB it was given every
  database's tables, because each MySQL database is a schema. Now its table list and `list_tables`
  cover the database in use (PostgreSQL: its schemas, the one in use first). The prompt names the
  database and schema and says to stay in it. With no MySQL database in use it gets only the
  database names and asks the user to pick one.
- Not enforced: a qualified name such as `otherdb.t` can still be written. Refusing that would be
  a `studio-safety` change.
- The chat shows "Working in server › database › schema", lists connections that way, and has
  database and schema pickers that use the same switch as the side panel (`lib/useNamespace.ts`).
  A switch mid-chat adds a note to the conversation. Conversations record their database, and the
  list shows "connection · database".
- Slip, corrected: running `npx prettier` on `AiTab.tsx` fetched Prettier with its defaults (the
  project has no config) and reflowed the file. It was reformatted with single quotes and 120
  columns, and the two untouched places that still differed were put back by hand.
- Checked: 345 Rust tests, 84 frontend tests, clippy, fmt, `cargo deny`, typecheck. The new
  end-to-end steps in `grid-and-diagram.mjs` have not run (no test servers).

## After hand-over: renamed to DB Studio, version 0.1.4
- The visible name is DB Studio everywhere (window, header, installer, assistant prompt, crash
  and diagnostics text, the name reported to servers, docs). npm package: `db-studio`.
- Decision: the identifier is now `com.dbstudio.desktop`, for the app data folder and the keychain
  service, so the app starts with no saved connections. Old data stays in
  `%APPDATA%\com.aidbstudio.app` and under `com.aidbstudio.app` in Credential Manager; nothing
  migrates it, and nothing deletes it.
- The MSI upgrade code is pinned to the old derived one (`C83131D2-971B-5CC4-8418-CC52CB698DE9`),
  so installing replaces an "AI DB Studio" install instead of sitting beside it.
- The legacy importer still looks for the old Electron app under its old name, on purpose.

## Verification at hand-over
- `cargo test --workspace` with every server up, including MySQL TLS: 250 passed, 0 failed, 0 skipped.
- `npm test`: 34 passed. Clippy, fmt and `cargo deny check` clean.
- End-to-end: `tests/e2e/` against the real app with embedded assets and the production CSP: 25/25 and
  19/19, using `STUDIO_DATA_DIR` so a developer's own data is left alone.
- Not run: installing the installers. CI has never run.
- Next: review the diff and commit; generate updater keys; run CI on macOS and Linux.
