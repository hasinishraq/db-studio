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

## Verification at hand-over
- `cargo test --workspace` with every server up, including MySQL TLS: 250 passed, 0 failed, 0 skipped.
- `npm test`: 34 passed. Clippy, fmt and `cargo deny check` clean.
- End-to-end: `tests/e2e/` against the real app with embedded assets and the production CSP: 25/25 and
  19/19, using `STUDIO_DATA_DIR` so a developer's own data is left alone.
- Not run: installing the installers. CI has never run.
- Next: review the diff and commit; generate updater keys; run CI on macOS and Linux.
