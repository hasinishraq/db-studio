# AI DB Studio (Rust) — Build Guide for Claude Code

Nov 7, 2026 · @FYDP III

## 1. How to use this guide with Claude Code

Build the app in 17 phases (0 to 16), one phase per Claude Code session. Each phase ends at a verification gate (commands that must pass) and a git commit. Never start a phase until the previous gate is green.

### Set-up, once

1. Create an empty folder for the project and run `git init` in it.
2. Export this document as Markdown and save it as `docs/BUILD_GUIDE.md` in that folder.
3. Copy the `CLAUDE.md` from section 4 into the folder root. Claude Code reads it automatically at the start of every session.
4. Create an empty `docs/PROGRESS.md`. Claude Code updates it at the end of every phase, so a new session knows where things stand.
5. Open a terminal in the folder and start Claude Code with `claude`.

### The loop for every phase

1. **Start fresh.** Run `/clear` (or open a new session) so the previous phase's context does not crowd this one.
2. **Paste the phase prompt** from section 7. Every prompt starts by telling Claude Code to read `CLAUDE.md`, `docs/BUILD_GUIDE.md` (that phase only) and `docs/PROGRESS.md`.
3. **Ask for a plan first.** Each prompt asks Claude Code to list the files it will create and the tests it will write, then wait. Read the plan. Correct it before any code is written.
4. **Let it implement**, including the tests listed in the phase.
5. **Run the gate yourself** (the commands under "Gate" in each phase). If anything fails, paste the error back and ask for a fix of the root cause.
6. **Review the diff** (`git diff`). Look especially at anything touching SQL safety, approvals or secrets.
7. **Commit**: `git add -A && git commit -m "phase N: <title>"`. Optionally push to a branch and open a pull request so CI runs.
8. Confirm `docs/PROGRESS.md` was updated with what was done, decisions made and known gaps.

### Golden rules when working with Claude Code

- **One phase at a time.** If it starts building something from a later phase, stop it.
- **Tests in the same phase**, never "later". A phase without its tests is not done.
- **Verify APIs, do not guess.** Crate APIs change between versions. Tell it to check `docs.rs` or run `cargo doc --open` for the exact version in `Cargo.lock` when unsure.
- **Use `cargo add`** to add dependencies so you get current compatible versions. The versions in this guide are a guide, not a pin.
- **Never weaken a safety test to make it pass.** If a classifier or approval test fails, the code is wrong, not the test.
- **Small commits.** If a change grows past a few hundred lines, ask it to split the work.
- **Two failed attempts at the same error = stop.** Ask it to explain the root cause and options before trying again.
- **Keep secrets out of the repo.** No passwords, API keys or signing keys in code, tests, fixtures or logs.

### Prompt to start any session

```text
Read CLAUDE.md, then docs/PROGRESS.md, then the section of docs/BUILD_GUIDE.md for Phase <N>.
Summarise in 5 lines what is already done and what Phase <N> must deliver.
Then propose a plan: files to create or change, public types and functions, tests to add.
Do not write code until I approve the plan.
```

### Prompt to end any session

```text
Run the Phase <N> gate commands and show me the output.
Then update docs/PROGRESS.md: what was built, decisions taken, deviations from the guide, known gaps, next phase.
Do not commit; I will review and commit.
```

## 2. What we are building

AI DB Studio is a desktop database client for PostgreSQL, MySQL/MariaDB and SQLite, with an AI assistant that runs safe reads on its own and can change data only after the user approves the exact statement. It is a Tauri 2 app: a Rust backend in a Cargo workspace, and a React + TypeScript frontend in the system webview.

### Scope of version 1.0

- Saved connection profiles, with TLS (verified by default) and SSH tunnels.
- Schema explorer, table structure, ER diagram.
- Monaco SQL editor with schema-aware completion, run selection, explain, cancel.
- Streaming virtual results grid (no row limit in memory), export CSV/JSON/XLSX/SQL, CSV import.
- Table data tab: server-side paging, sorting, filtering; insert, edit and delete rows through reviewed SQL.
- Manual transaction mode; read-only and production profiles with coloured title bar.
- AI chat (Ollama, OpenAI and compatible) with tool calling, review cards for writes, charts, memory.
- Audit log of every write; secrets in the OS keychain; optional encrypted local database.
- Signed installers and auto-update for macOS, Windows and Linux.

### Out of scope for 1.0

- Cloud sync, accounts or telemetry.
- Engines other than PostgreSQL, MySQL/MariaDB and SQLite.
- Cross-database queries in one chat.

### Crate map

| Crate | Folder | Job | Depends on |
| --- | --- | --- | --- |
| `studio-types` | `crates/studio-types` | DTOs shared with the frontend (serde + specta), `AppError` | none |
| `studio-db` | `crates/studio-db` | `Driver` / `Session` traits, `Value`, `RowBatch`, `DbError` | studio-types |
| `driver-postgres` | `crates/driver-postgres` | PostgreSQL via `tokio-postgres` + `deadpool-postgres` + rustls | studio-db |
| `driver-mysql` | `crates/driver-mysql` | MySQL / MariaDB via `mysql_async` + rustls | studio-db |
| `driver-sqlite` | `crates/driver-sqlite` | SQLite via `rusqlite` (bundled) on blocking threads | studio-db |
| `studio-safety` | `crates/studio-safety` | `sqlparser` classifier, risk reasons, approval tokens | studio-types |
| `studio-tunnel` | `crates/studio-tunnel` | SSH local port forwarding via `russh`, known-hosts | studio-types |
| `studio-session` | `crates/studio-session` | Open sessions, runs, streaming, cancel, transactions, policy | studio-db, studio-safety, studio-tunnel |
| `studio-store` | `crates/studio-store` | App SQLite (`sqlx` + migrations), keyring secrets, audit log | studio-types |
| `studio-ai` | `crates/studio-ai` | Agent loop, tools, providers via `rig-core`, memory, offline engine | studio-session, studio-safety, studio-store |
| `studio-app` | `src-tauri` | Tauri binary: state, commands, plugins, bindings export | all of the above |

The frontend lives in `src/` and talks to Rust only through `src/bindings.ts`, which `tauri-specta` generates from the Rust commands.

### Rules the architecture enforces

- Drivers know nothing about Tauri, AI or storage, so each can be tested alone against a container.
- `studio-ai` reaches databases only through `studio-session`, which applies the safety rules. There is no path from the model to a raw connection.
- The webview never receives a secret, never decides whether SQL is safe, and never holds an approval it did not get from a user click.

The drawing below shows how the layers call each other.

&#91;embedded content: crate architecture · dashed boxes are outside the app\]

The frontend and the app layer sit on top; studio-session (highlighted) is the only crate that runs SQL, and it consults studio-safety before every statement. studio-types is used by every crate and is left out for clarity.

## 3. Prerequisites and environment setup

You need Rust (stable), Node.js 22 or newer, the Tauri system dependencies for your OS, Docker (for test databases), and Claude Code. Install them before Phase 0; the gate at the end of this section checks them.

### All platforms

| Tool | Why | Install |
| --- | --- | --- |
| Rust stable + clippy + rustfmt | Backend | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` (Windows: rustup-init.exe from rustup.rs) |
| Node.js 22 LTS or newer + npm | Frontend build | nodejs.org, or `nvm install 22` |
| Tauri CLI 2 | `cargo tauri dev/build` | `cargo install tauri-cli --version "^2" --locked` |
| Docker Desktop / Docker Engine | PostgreSQL, MySQL, SSH test containers | docker.com |
| Git | Version control | git-scm.com |
| Claude Code | Builds the project with you | `npm install -g @anthropic-ai/claude-code` |
| cargo-nextest (optional) | Faster test runner | `cargo install cargo-nextest --locked` |
| cargo-deny | Licence and advisory checks | `cargo install cargo-deny --locked` |
| sqlx-cli (optional) | Create and run migrations by hand | `cargo install sqlx-cli --no-default-features --features sqlite,rustls --locked` |

### macOS

```bash
xcode-select --install          # compilers and SDK
rustup target add aarch64-apple-darwin x86_64-apple-darwin   # for universal builds later
```

### Windows 10/11

1. Install **Visual Studio Build Tools** with the workload **Desktop development with C++** (MSVC + Windows SDK).
2. **WebView2** is preinstalled on Windows 10 (recent updates) and 11. If not, install the Evergreen runtime from Microsoft.
3. Use the MSVC Rust toolchain: `rustup default stable-x86_64-pc-windows-msvc`.
4. If a crate using `aws-lc-rs` fails to build, install CMake and NASM, or switch that crate to the `ring` backend (section 11).

### Linux (Debian/Ubuntu)

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
# keyring tests against the real store need a Secret Service (GNOME Keyring or KWallet)
sudo apt install -y gnome-keyring libdbus-1-dev pkg-config
```

Fedora, Arch and others: use the package names from the Tauri v2 prerequisites page for your distribution.

### Gate: environment check

```bash
rustc --version && cargo --version && cargo clippy --version && cargo fmt --version
node --version && npm --version
cargo tauri --version
docker run --rm hello-world
claude --version
```

All commands must print a version (Docker prints its hello message). Node must be 22 or newer; Tauri CLI must be 2.x.

## 4. CLAUDE.md (put this file in the repo root)

Copy the block below exactly into `CLAUDE.md`. It is short on purpose: Claude Code loads it every session, and the detail lives in `docs/BUILD_GUIDE.md`.

```markdown
# AI DB Studio (Rust) - instructions for Claude Code

## What this is
Desktop database client (PostgreSQL, MySQL/MariaDB, SQLite) with a safe AI assistant.
Tauri 2 + Rust workspace backend, React + TypeScript frontend in src/.
Full plan: docs/BUILD_GUIDE.md. Current state: docs/PROGRESS.md. Read both before working.

## How to work
- Work on ONE phase of docs/BUILD_GUIDE.md at a time. Do not build features from later phases.
- Always propose a plan (files, public API, tests) and wait for approval before writing code.
- Every phase ships with its tests. Run the phase gate before saying you are done.
- When unsure of a crate API, check docs.rs for the version in Cargo.lock or run `cargo doc`. Never guess.
- Add dependencies with `cargo add` (workspace deps go in the root [workspace.dependencies]).
- If the same error survives two attempts, stop and explain the root cause and options.
- At the end of a phase, update docs/PROGRESS.md (done, decisions, deviations, gaps, next).
- Never commit; the human reviews and commits.

## Layout
- crates/studio-types    DTOs + AppError (serde + specta). No logic.
- crates/studio-db       Driver/Session traits, Value, RowBatch, DbError.
- crates/driver-*        One engine each. Depend only on studio-db (+ studio-types).
- crates/studio-safety   SQL classification (sqlparser) + approval tokens. Pure, no I/O.
- crates/studio-tunnel   SSH tunnels (russh).
- crates/studio-session  Sessions, runs, streaming, cancel, transactions, policy.
- crates/studio-store    App SQLite (sqlx), migrations, keyring secrets, audit log.
- crates/studio-ai       Agent loop, tools, providers (rig-core), memory.
- src-tauri              Tauri app: AppState, commands, plugins, bindings export.
- src                    React app. Calls Rust ONLY via src/bindings.ts (generated; never edit).

## Commands
- Dev app:            cargo tauri dev
- Rust tests:         cargo test --workspace   (DB tests need: docker compose -f docker/compose.yml up -d)
- Lint:               cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
- Frontend:           npm run typecheck && npm test
- Regenerate bindings: cargo test -p studio-app export_bindings (or run the app in debug once)
- Licences/advisories: cargo deny check

## Non-negotiable safety rules
1. The webview is untrusted. Every command re-validates its input in Rust.
2. Secrets (passwords, passphrases, API keys) live only in the OS keychain via studio-store.
   Never return a secret to the frontend, never log it, never put it in a DTO.
3. Only studio-safety decides whether SQL is read-only. The frontend never decides.
4. SQL that does not parse is treated as a write (fail closed).
5. Any non-read statement needs an approval token: random, single use, 60 s, bound to
   sha256(exact SQL) + session id. Consume (remove) the token BEFORE comparing.
6. Approval tokens are minted ONLY by commands triggered by a human click
   (approve_query, ai_review). No other code path may mint one.
7. AI-generated SQL runs without review only if studio-safety says ai_auto = true,
   and then always inside a READ ONLY transaction with a timeout and row cap.
8. Read-only profiles refuse every non-read statement, even with an approval.
9. Result rows are never sent to the AI model unless the user enabled sharing, and are
   never stored in AI memory or logs.
10. TLS certificate verification is ON by default. Disabling it requires an explicit profile mode.
11. Never weaken, skip or delete a safety test to make it pass.

## Code style
- Rust 2021 edition, stable toolchain. No `unsafe`. No `unwrap()`/`expect()` outside tests
  (except documented invariants at startup).
- Library crates: errors with `thiserror`. The app maps them to AppError { code, message }.
- Async everywhere with tokio. Blocking work (rusqlite, keyring, file I/O) in spawn_blocking.
- Never hold a lock across an .await that does network I/O.
- Logging with `tracing`. Never log SQL parameters, row values or secrets.
- Public items get a doc comment. Keep functions small; prefer explicit types at crate boundaries.
- Frontend: TypeScript strict, no `any` in new code, components small, state in contexts.
```

## 5. Project bootstrap (detail for Phase 0)

The end state of bootstrap is a Tauri 2 app that opens an empty React window, inside a Cargo workspace with all crate folders created and CI-ready lint settings. Phase 0 in section 7 asks Claude Code to do exactly these steps.

### Step 1: scaffold Tauri + React

Scaffold into a temporary folder, then move the files into your repo (which already holds `CLAUDE.md` and `docs/`).

```bash
cd ..
npm create tauri-app@latest
#   Project name:        ai-db-studio-scaffold
#   Identifier:          com.aidbstudio.app   (choose your own reverse-domain id)
#   Frontend language:   TypeScript / JavaScript
#   Package manager:     npm
#   UI template:         React
#   UI flavor:           TypeScript
cp -R ai-db-studio-scaffold/. ai-db-studio/
rm -rf ai-db-studio-scaffold
cd ai-db-studio
npm install
cargo tauri dev      # an empty window must open
```

### Step 2: turn it into a workspace

1. Create the root `Cargo.toml` from appendix A.1 (workspace members `crates/*` and `src-tauri`).
2. Rename the package in `src-tauri/Cargo.toml` to `studio-app` and its lib to `studio_app_lib`, and switch shared dependencies to `{ workspace = true }`.
3. Create each library crate: `cargo new --lib crates/<name>` for studio-types, studio-db, driver-postgres, driver-mysql, driver-sqlite, studio-safety, studio-tunnel, studio-session, studio-store, studio-ai.
4. In every crate's `Cargo.toml` set `edition.workspace = true` and `license.workspace = true`.

### Step 3: repo hygiene files

| File | Content |
| --- | --- |
| `rust-toolchain.toml` | `[toolchain]` `channel = "stable"`, `components = ["clippy", "rustfmt"]` |
| `rustfmt.toml` | `max_width = 110`, `edition = "2021"` |
| `.cargo/config.toml` | optional: `[alias] t = "test --workspace"`, `lint = "clippy --workspace --all-targets -- -D warnings"` |
| `deny.toml` | from `cargo deny init`; allow MIT, Apache-2.0, BSD-2/3, ISC, Unicode, Zlib, MPL-2.0 |
| `.gitignore` | `target/`, `node_modules/`, `dist/`, `.env*`, `*.db`, `src-tauri/gen/` (keep `gen/schemas` if you prefer it committed) |
| `docker/compose.yml` | test databases and SSH server (appendix A.4) |
| `docs/PROGRESS.md` | empty, filled at the end of each phase |
| `.github/workflows/ci.yml` | lint + tests (appendix A.6) |

In the root `Cargo.toml`, add workspace lints so every crate shares them:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
unwrap_used = "warn"
expect_used = "warn"
dbg_macro = "deny"
todo = "warn"
```

Each crate then adds `[lints] workspace = true`.

### Step 4: frontend dependencies

Keep the same UI stack as the Electron version so components can be ported with little change:

```bash
npm install react@19 react-dom@19 lucide-react @tanstack/react-table @tanstack/react-virtual \
  @tanstack/react-query monaco-editor sql-formatter react-markdown remark-gfm \
  react-resizable-panels cmdk clsx tailwind-merge class-variance-authority \
  @radix-ui/react-dialog @radix-ui/react-alert-dialog @radix-ui/react-dropdown-menu \
  @radix-ui/react-context-menu @radix-ui/react-popover @radix-ui/react-select \
  @radix-ui/react-tabs @radix-ui/react-tooltip @radix-ui/react-slot \
  @tauri-apps/api @tauri-apps/plugin-dialog @tauri-apps/plugin-updater \
  @tauri-apps/plugin-process @tauri-apps/plugin-opener
npm install -D tailwindcss@3 postcss autoprefixer vitest @testing-library/react jsdom typescript
```

Tailwind 3 is used on purpose: the Electron version's `tailwind.config.js` and classes copy over unchanged.

Add these scripts to `package.json`:

```json
{
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "typecheck": "tsc --noEmit",
    "test": "vitest run",
    "tauri": "tauri"
  }
}
```

### Step 5: Vite settings for Tauri

```ts
// vite.config.ts
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,                       // keep Rust errors visible
  server: { port: 1420, strictPort: true }, // must match build.devUrl in tauri.conf.json
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  resolve: { alias: { '@': path.resolve(__dirname, 'src') } },
  build: { target: 'es2022', sourcemap: true },
});
```

### Step 6: final folder layout

```text
ai-db-studio/
├── CLAUDE.md
├── Cargo.toml            workspace
├── rust-toolchain.toml   rustfmt.toml   deny.toml
├── crates/
│   ├── studio-types/  studio-db/  driver-postgres/  driver-mysql/  driver-sqlite/
│   ├── studio-safety/  studio-tunnel/  studio-session/  studio-store/  studio-ai/
├── src-tauri/
│   ├── Cargo.toml  build.rs  tauri.conf.json
│   ├── capabilities/main.json
│   ├── icons/
│   └── src/ main.rs  lib.rs  state.rs  error.rs  commands/
├── src/                  React app (bindings.ts generated here)
├── docker/compose.yml
├── docs/ BUILD_GUIDE.md  PROGRESS.md
└── .github/workflows/ ci.yml  release.yml
```

## 6. Coding standards and non-negotiable security rules

The `CLAUDE.md` rules are the short form; this section is the full form Claude Code should follow when a rule is unclear.

### Errors

- Each library crate has one error enum with `thiserror`, for example `DbError`, `SafetyError`, `StoreError`, `TunnelError`, `AiError`.
- The app crate converts every error to `AppError`, which is what commands return:

```rust
#[derive(Debug, serde::Serialize, specta::Type)]
pub struct AppError {
    pub code: ErrorCode,          // stable, machine-readable
    pub message: String,          // human-readable, no secrets
    pub detail: Option<DbErrorDetail>, // SQLSTATE, position, hint for the editor
}

#[derive(Debug, serde::Serialize, specta::Type)]
pub enum ErrorCode {
    NotConnected, ApprovalRequired, ApprovalExpired, ApprovalMismatch, ReadOnlyProfile,
    Timeout, Cancelled, Database, Tls, Ssh, HostKeyChanged, Secret, Ai, InvalidInput, Internal,
}
```

- Never put a password, connection string with password, API key or row value into an error message.

### Async and concurrency

- Commands are `async fn`. Long work is a spawned task with a `CancellationToken`.
- `rusqlite`, `keyring`, large file writes: inside `tokio::task::spawn_blocking`.
- All `keyring` calls go through one dedicated worker (a single thread with a channel), because some OS stores misbehave under concurrent access.
- Use `tokio::sync` locks, never `std::sync::Mutex` across `.await`. Prefer `DashMap` for id-keyed maps.
- Bounded channels for row streaming (for example capacity 8 batches) so a slow UI applies backpressure to the database read.

### IPC conventions

- Command names are `snake_case` verbs: `list_profiles`, `run_query`, `ai_review`.
- Every command and DTO derives `specta::Type`; bindings are generated, never hand-written.
- IDs are newtypes (`SessionId(Uuid)`, `RunId(Uuid)`, `ProfileId(Uuid)`) serialized as strings.
- Streams use `tauri::ipc::Channel<T>`, one per call. Global events only for app-wide news (connection lost, update available).
- Validate every input in Rust: lengths (SQL at most 1 MB from the editor), enum values, ids that must exist.

### Logging

- `tracing` with spans per command and per run (`run_id`, `session_id`, `engine`, duration, row count, error code).
- Never log: SQL parameters, row values, secrets, full SQL text of AI turns unless the user enables debug logging.

### Tests

- Unit tests next to the code (`#[cfg(test)] mod tests`). Integration tests in each crate's `tests/` folder.
- Database integration tests read `TEST_PG_URL` and `TEST_MYSQL_URL`; when unset they are skipped with a clear message, never silently passing.
- Safety tests are table-driven: one row per SQL string with the expected verdict. Add a row for every bug found.
- Snapshot tests (`insta`) for generated SQL (filters, row edits, DDL reconstruction).

### Frontend

- TypeScript `strict: true`. No `any` in new code.
- All Rust calls go through `src/api/` wrappers over `bindings.ts`; components never import `@tauri-apps/api` directly except `Channel`.
- No `dangerouslySetInnerHTML`. Markdown from the AI is rendered with `react-markdown`; links open via the opener plugin only for `https:` and `mailto:`.

### Security rules (expanded)

| # | Rule | Where enforced | Test that proves it |
| --- | --- | --- | --- |
| 1 | Webview untrusted; Rust re-validates all input | every command | invalid-input tests per command |
| 2 | Secrets only in keychain, never returned to UI | studio-store, DTOs | DTO has no secret fields; profile round-trip test |
| 3 | Only studio-safety classifies SQL | studio-session | frontend has no classifier logic (code review) |
| 4 | Unparseable SQL = write | studio-safety | parse-failure rows in classifier table |
| 5 | Writes need a single-use, 60 s token bound to SQL hash + session | studio-safety, studio-session | reuse, expiry, other-SQL, other-session tests |
| 6 | Tokens minted only from human-click commands | studio-app | grep test: `ApprovalBook::issue` called only in approve\_query and ai\_review |
| 7 | AI auto-reads: strict verdict + READ ONLY transaction + timeout + cap | studio-session | safety suite against real databases |
| 8 | Read-only profiles refuse writes even with a token | studio-session | policy test |
| 9 | Rows to the model only when shared; never in memory or logs | studio-ai | agent tests with a fake provider |
| 10 | TLS verification on by default | drivers | self-signed server rejected in verify-full |
| 11 | SSH host key pinned; change is a hard error | studio-tunnel | changed-key test |

### Commit messages

`phase N: <short title>` for phase commits; `fix(<crate>): ...`, `feat(<crate>): ...`, `test(<crate>): ...` for smaller ones.

## 7. Phase-by-phase build plan

Seventeen phases take the project from an empty window to a signed release; each lists its goal, files, tasks, gate and a prompt to paste into Claude Code. Backend phases 1 to 8 come first so the frontend is built on a tested core. Day estimates assume one developer working with Claude Code and are only a guide.

| Phase | Delivers | Needs | Days |
| --- | --- | --- | --- |
| 0 | Bootstrap: workspace, empty Tauri window, lint, CI, test containers | none | 1-2 |
| 1 | `studio-types`: DTOs, ids, `AppError`, `Value` | 0 | 1-2 |
| 2 | `studio-store`: app SQLite, migrations, keychain secrets, audit log | 1 | 3-4 |
| 3 | `studio-db`: driver traits, conformance test kit | 1 | 2 |
| 4 | `driver-postgres` with TLS, streaming, cancel | 3 | 5-7 |
| 5 | `driver-mysql` with TLS, streaming, cancel | 3 | 4-6 |
| 6 | `studio-safety`: parser classifier, approvals | 1 | 4-5 |
| 7 | `studio-session`: runs, policy, transactions | 4, 5, 6 | 4-5 |
| 8 | `studio-app`: state, commands, bindings, capabilities | 2, 7 | 3-4 |
| 9 | Frontend shell, connections, explorer | 8 | 4-5 |
| 10 | SQL editor, run flow, streaming grid, history | 9 | 5-6 |
| 11 | Table data, structure, ER diagram | 10 | 4-5 |
| 12 | `studio-ai` backend: agent, tools, memory | 7, 2 | 5-6 |
| 13 | AI frontend: chat, review cards, charts, settings | 12, 10 | 4-5 |
| 14 | SSH tunnels and certificate management | 7, 9 | 4-5 |
| 15 | Production features: transactions UI, prod profiles, audit UI, export/import, explain, masking, SQLite | 11, 13 | 8-10 |
| 16 | Release: data import from Electron, signing, updater, CI release, hardening | all | 4-6 |

---

### Phase 0 - Bootstrap

**Goal.** A Cargo workspace plus a Tauri 2 app that opens an empty React window, with lint, CI and test databases ready.

**Files.** Everything listed in section 5, steps 1 to 6; `docker/compose.yml` (appendix A.4); `.github/workflows/ci.yml` (appendix A.6).

**Tasks.**

1. Follow section 5 exactly.
2. Each library crate compiles with an empty `lib.rs` and one passing test.
3. `docker compose -f docker/compose.yml up -d` starts PostgreSQL 17, MySQL 8.4 and an SSH server, all healthy.
4. Write `docs/PROGRESS.md` with a "Phase 0 done" entry.

**Gate.**

```bash
cargo build --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run typecheck
docker compose -f docker/compose.yml ps     # all services healthy
cargo tauri dev                            # empty window opens; close it
```

**Prompt.**

```text
Read CLAUDE.md and docs/BUILD_GUIDE.md sections 3, 5 and Phase 0 of section 7.
Goal: bootstrap the repo exactly as section 5 describes (Tauri 2 + React TS scaffold,
Cargo workspace with all crates under crates/, src-tauri renamed to studio-app,
workspace lints, rust-toolchain.toml, rustfmt.toml, deny.toml, .gitignore,
docker/compose.yml from appendix A.4, .github/workflows/ci.yml from appendix A.6).
First show me the plan and the list of files. Wait for approval.
After implementing, run every Phase 0 gate command and show the output.
```

---

### Phase 1 - Shared types (`studio-types`)

**Goal.** Every type that crosses the IPC boundary, defined once, serializable and exported to TypeScript.

**Files.** `crates/studio-types/src/{lib.rs, ids.rs, profile.rs, value.rs, query.rs, schema.rs, ai.rs, settings.rs, error.rs}`.

**Tasks.**

1. Id newtypes over `uuid::Uuid` (serialized as strings): `ProfileId`, `SessionId`, `RunId`, `QueryId`, `ConversationId`, `TurnId`.
2. `Engine { Postgres, MySql, Sqlite }`, `TlsMode { Disable, Require, VerifyCa, VerifyFull }`, `Environment { Development, Staging, Production }`, `AiPolicy { Default, LocalOnly, NoRowSharing, Disabled }`.
3. `ConnectionProfile`: id, name, engine, host, port, database, username, tls\_mode, ca\_cert\_path, client\_cert\_path, ssh (`Option<SshConfig>`), environment, read\_only, ai\_policy, color, has\_password (bool, set by the store). **No password field.**
4. `SshConfig`: host, port, username, auth (`SshAuth { Password, PrivateKey { path }, Agent }`), has\_secret.
5. `Value` enum exactly as in the architecture document (Null, Bool, Int, BigInt(String), Float, Decimal(String), Text, Json(String), Bytes { len, preview\_hex }, Temporal(String), Unsupported(String)), serde tag `t` / content `v`.
6. `ColumnMeta { name, type_name, nullable: Option<bool> }`, `RowBatch { seq, rows }`, `StatementOutcome` (Rows { columns, total\_rows, truncated, elapsed\_ms } | Command { rows\_affected, elapsed\_ms } | Error { error }), `ExecSummary`.
7. Schema DTOs: `SchemaRef`, `DbObject { name, schema, kind: Table | View | MaterializedView }`, `ColumnInfo`, `TableStructure` (columns, indexes, foreign keys, constraints, metadata, ddl).
8. `FilterRule { column, operator, value }` with the 11 operators from the Electron app; `SortSpec`.
9. Verdict DTOs shown to the UI: `StatementVerdict { span, kind, read_only, risk_reasons }`.
10. AI DTOs: `AiEvent` (Text, QueryStart, QueryApproved, QueryDeclined, QueryCancelled, QueryResult, QueryError, Done), `AiSendRequest`.
11. `Settings` (theme, history\_limit, ai\_provider, ai\_endpoint, ai\_model, ai\_share\_rows, auto\_run\_reads, encrypt\_store) with defaults.
12. `AppError` + `ErrorCode` from section 6.
13. Derive `Serialize, Deserialize, Clone, Debug, specta::Type` everywhere; `#[serde(rename_all = "camelCase")]` on structs.

**Tests.** serde round-trip for every enum and struct; `Value` JSON shape snapshot (`insta`); a test asserting `ConnectionProfile` JSON contains no `password` key.

**Gate.** `cargo test -p studio-types && cargo clippy -p studio-types -- -D warnings`

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 1 of docs/BUILD_GUIDE.md section 7.
Implement crates/studio-types exactly as listed: id newtypes, enums, ConnectionProfile
(no password field), SshConfig, Value, result types, schema DTOs, filters, verdict DTOs,
AI events, Settings, AppError/ErrorCode. Use serde camelCase and derive specta::Type.
Add the listed tests including insta snapshots of Value JSON.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 2 - App storage and secrets (`studio-store`)

**Goal.** A local SQLite database with migrations for all app data, secrets in the OS keychain, and a tamper-evident audit log.

**Files.** `crates/studio-store/src/{lib.rs, db.rs, profiles.rs, drafts.rs, history.rs, settings.rs, conversations.rs, audit.rs, known_hosts.rs, secrets.rs}`, `crates/studio-store/migrations/0001_init.sql`.

**Tasks.**

1. `Store::open(dir: &Path)` creates `studio.db`, enables WAL and foreign keys, and runs embedded migrations (`sqlx::migrate!`). File permissions owner-only on Unix.
2. Migration `0001_init.sql`: tables `profiles`, `drafts`, `history`, `settings`, `conversations`, `turns`, `audit_log`, `known_hosts`, `masking_rules` (schema in the architecture document, plus `masking_rules(profile_id, schema_name, table_name, column_name)`).
3. Use runtime-checked queries (`sqlx::query_as::<_, Row>` with `FromRow`) so builds never need a live database. Do not use the `query!` macros unless you also commit `.sqlx` offline data.
4. Repositories with async methods: profiles (list, get, upsert, delete), drafts, history (add, list with limit, clear, trim to `history_limit`), settings (get/set typed `Settings`), conversations and turns (append, list, forget\_from, clear, set\_summary), known\_hosts (get, trust, replace).
5. `SecretStore` trait: `get`, `set`, `delete` by `SecretKey` (`ProfilePassword(ProfileId)`, `SshSecret(ProfileId)`, `AiKey(provider)`, `StoreKey`). Implementations: `KeyringSecretStore` (crate `keyring` with `apple-native`, `windows-native`, `sync-secret-service`) running on one dedicated worker thread, and `MemorySecretStore` used when the OS store is unavailable and in tests.
6. Secrets are returned as `secrecy::SecretString`. `ConnectionProfile.has_password` is filled from the secret store; the password itself never leaves the crate except to the session manager.
7. Audit log: `append(entry)` computes `entry_hash = sha256(prev_hash || canonical_json(entry))`; `verify_chain()` returns the first broken id, if any.

**Tests.** temp-dir store per test (`tempfile`); migration from empty; profile CRUD; history trimming; conversation forget\_from; audit chain verifies, and detects a tampered row; secret store tests with `MemorySecretStore` (and the keyring crate's mock store), never the real OS store in CI.

**Gate.** `cargo test -p studio-store && cargo clippy -p studio-store -- -D warnings`

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 2 of docs/BUILD_GUIDE.md section 7.
Implement crates/studio-store: Store::open with WAL + embedded sqlx migrations,
0001_init.sql with all listed tables, runtime-checked repositories, SecretStore trait
with KeyringSecretStore (single worker thread) and MemorySecretStore, and the
hash-chained audit log with verify_chain. Secrets use secrecy::SecretString and are
never placed in DTOs. Tests must use temp dirs and the memory/mock secret store.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 3 - Driver traits and conformance kit (`studio-db`)

**Goal.** One engine-neutral interface for every database, plus a shared test suite every driver must pass.

**Files.** `crates/studio-db/src/{lib.rs, driver.rs, session.rs, config.rs, error.rs, sql.rs}`, `crates/studio-db/src/conformance/{mod.rs, metadata.rs, execute.rs, cancel.rs, readonly.rs, page.rs}` behind a `conformance` feature.

**Tasks.**

1. `Driver` and `Session` traits exactly as in the architecture document (use `async_trait` so they are object-safe): `server_info`, `list_schemas`, `list_objects`, `columns`, `table_structure`, `execute`, `table_page`, `table_count`, `explain`, `begin`, `commit`, `rollback`, `canceller`, `close`.
2. `ConnectConfig` built by the session manager: host, port, database, username, password (`SecretString`), tls (mode, CA path, client cert/key paths), connect timeout, application name `AI DB Studio`.
3. `ExecOptions { read_only_tx, timeout, max_rows, batch_size }` and `RowSink = tokio::sync::mpsc::Sender<RowBatch>`.
4. `Canceller` trait: `async fn cancel(&self) -> Result<(), DbError>`.
5. `DbError` with variants: Connect, Tls, Auth, Query { sqlstate, message, position }, Timeout, Cancelled, ReadOnlyViolation, Unsupported, Internal.
6. Helpers in `sql.rs`: `quote_ident(engine, name)` (double the quote character), `qualified(engine, schema, table)`, and a `FilterBuilder` that turns `FilterRule`s into a WHERE clause with bound parameters (`$1..` for PostgreSQL, `?` for MySQL/SQLite). Port the logic and tests from the Electron `buildWhere`.
7. Conformance kit: `pub async fn run_all(driver: &dyn Driver, cfg: ConnectConfig)` that creates its own scratch schema/tables, then checks: metadata lists them; `execute` streams 10 000 rows in batches in order; types round-trip (int, bigint, numeric, text, bool, json, date, timestamp, bytea/blob, uuid where supported, NULL); `max_rows` truncates and reports it; `read_only_tx` rejects `INSERT`; timeout fires; cancel stops a long query within 2 s; `table_page` sorts, filters, pages; `table_count` respects its timeout.

**Tests.** unit tests for `quote_ident` and `FilterBuilder`, including an injection attempt in a filter value; snapshot tests of generated WHERE clauses.

**Gate.** `cargo test -p studio-db --all-features && cargo clippy -p studio-db --all-features -- -D warnings`

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 3 of docs/BUILD_GUIDE.md section 7.
Implement crates/studio-db: object-safe Driver/Session/Canceller traits (async_trait),
ConnectConfig, ExecOptions, RowSink, DbError, quote_ident/qualified helpers, and
FilterBuilder with bound parameters per dialect. Then build the conformance kit behind
the `conformance` feature as listed; it must create and drop its own scratch objects.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 4 - PostgreSQL driver (`driver-postgres`)

**Goal.** A PostgreSQL driver that passes the conformance kit, with verified TLS, streaming rows, protocol-level cancel and read-only transactions.

**Files.** `crates/driver-postgres/src/{lib.rs, connect.rs, tls.rs, decode.rs, metadata.rs, execute.rs, page.rs}`, `crates/driver-postgres/tests/conformance.rs`.

**Crates.** `tokio-postgres`, `deadpool-postgres`, `tokio-postgres-rustls`, `rustls`, `rustls-native-certs`, `rustls-pemfile`, `chrono`, `rust_decimal` (or keep numeric as text), `serde_json`, `uuid`, `futures`.

**Tasks.**

1. **TLS** (`tls.rs`): build a `rustls::ClientConfig` per mode. VerifyFull = system roots + optional CA file + hostname check. VerifyCa = chain check without hostname (custom verifier that delegates to the webpki verifier and ignores only the name mismatch). Require = encrypt without verification (custom verifier that accepts any certificate; show a warning in the UI). Disable = `NoTls`. Install a rustls crypto provider once at startup (section 11).
2. **Connect**: `tokio_postgres::Config` from `ConnectConfig` (connect timeout 8 s, keepalives on, `application_name`). Pool of 4 with `deadpool-postgres`. Test the pool by running `SELECT version()`; on failure close it and return the error.
3. **Pinned connection for transactions**: manual-commit mode takes one client out of the pool and keeps it until commit/rollback.
4. **Execute**: prepare the statement, read its column metadata, then `query_raw` to get a `RowStream`. Decode each row into `Vec<Value>` and send batches of `batch_size`. Stop reading at `max_rows` and mark `truncated`. For statements without columns, return `Command { rows_affected }`.
5. **Decoding** (`decode.rs`): map by type OID. bool, int2/int4 to `Int`; int8, numeric to `BigInt`/`Decimal` as text; float4/8; text/varchar/bpchar/name/citext; json/jsonb to `Json`; uuid; bytea to `Bytes` (length + first 64 bytes hex); date/time/timestamp/timestamptz/interval to `Temporal` with the database's own text form (no timezone conversion); arrays and composites to `Json` text; enums and unknown text-like types through a custom `FromSql` that accepts any type and reads the raw text bytes when the format is text-compatible; anything else `Unsupported(type_name)`.
6. **Read-only**: when `read_only_tx`, run `BEGIN READ ONLY`, `SET LOCAL statement_timeout = <ms>`, the statement, then always `ROLLBACK` (in a guard that runs on error and cancel too).
7. **Timeout** (non read-only): `SET statement_timeout` for that call and reset afterwards, or wrap in `tokio::time::timeout` and cancel on expiry.
8. **Cancel**: keep the client's `CancelToken`; `Canceller::cancel` calls `cancel_query` with the same TLS connector.
9. **Metadata**: port the queries from the Electron `ConnectionManager.ts` (schemas, tables/views/materialized views, all columns with primary keys, indexes from `pg_indexes`, foreign keys, `pg_constraint`, size/owner/row estimate from `pg_class`, view DDL from `pg_get_viewdef`). Keep schema-qualified names.
10. **Paging**: `SELECT * FROM <qualified> <where> <order> LIMIT n OFFSET m` with bound filter values; count with `BEGIN READ ONLY` + 5 s `statement_timeout`, returning `None` on timeout.
11. **Explain**: `EXPLAIN (FORMAT JSON) <stmt>` for a single statement; never ANALYZE from this path.

**Tests.** `tests/conformance.rs` runs `studio_db::conformance::run_all` against `TEST_PG_URL` (skip with a message if unset); a TLS test against the TLS-enabled container: VerifyFull fails with an unknown CA, passes with the CA file; DATE round-trips unchanged in a non-UTC `TZ`.

**Gate.**

```bash
docker compose -f docker/compose.yml up -d postgres postgres-tls
TEST_PG_URL=postgres://studio:studio@127.0.0.1:54329/studio cargo test -p driver-postgres
cargo clippy -p driver-postgres -- -D warnings
```

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 4 of docs/BUILD_GUIDE.md section 7.
Implement crates/driver-postgres implementing studio-db's traits with tokio-postgres,
deadpool-postgres and rustls (all four TLS modes, verify-full by default), streaming
rows via query_raw in batches, OID-based decoding to Value (dates as server text),
read-only transactions with guaranteed ROLLBACK, statement timeouts, CancelToken-based
cancel, metadata queries ported from the Electron ConnectionManager, paging/count and
EXPLAIN (FORMAT JSON). Verify tokio-postgres and rustls APIs on docs.rs for the
versions in Cargo.lock. Make it pass the conformance kit and the TLS tests.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 5 - MySQL / MariaDB driver (`driver-mysql`)

**Goal.** The same contract for MySQL 8.x and MariaDB, passing the conformance kit.

**Files.** `crates/driver-mysql/src/{lib.rs, connect.rs, tls.rs, decode.rs, metadata.rs, execute.rs, page.rs}`, `crates/driver-mysql/tests/conformance.rs`.

**Crates.** `mysql_async` (with its rustls feature, not native-tls), `chrono`, `serde_json`, `futures`.

**Tasks.**

1. **Connect** with `OptsBuilder`: host, port, user, password, database (optional), TLS options per mode (verify-full: CA + hostname; verify-ca: skip domain validation; require: accept invalid certs; disable: no TLS), pool constraints min 0 max 4, connect timeout 8 s.
2. **Schemas** = databases: `SHOW DATABASES` minus system ones (`mysql`, `sys`, `performance_schema`, `information_schema` hidden by default). The explorer can browse every database the user can see, not only the profile's default.
3. **Execute** with `query_iter`, reading column metadata first, streaming rows in batches, honouring `max_rows`. Treat results without columns as `Command { rows_affected }`.
4. **Decoding**: by column type and flags. Integers to `Int`/`BigInt` (unsigned BIGINT as text); DECIMAL as `Decimal` text; FLOAT/DOUBLE; DATE/DATETIME/TIMESTAMP/TIME as `Temporal` text exactly as the server sent it; JSON; BLOB/BINARY to `Bytes`; BIT; ENUM/SET as text; everything else text or `Unsupported`.
5. **Read-only**: `SET SESSION MAX_EXECUTION_TIME = <ms>` (applies to SELECT), `START TRANSACTION READ ONLY`, statement, `ROLLBACK`, then reset `MAX_EXECUTION_TIME = 0`, all in a guard.
6. **Cancel**: record the connection id (`conn.id()`); `cancel` opens a second connection from the pool and runs `KILL QUERY <id>`.
7. **Metadata**: port the MySQL queries from the Electron app (`information_schema.columns`, `SHOW INDEX` with the table name escaped, `key_column_usage`, `check_constraints`, `SHOW CREATE TABLE`/`VIEW`). Always escape backticks in identifiers.
8. **Paging and count**: `LIMIT ? OFFSET ?` is not allowed with bound parameters in every server version, so inline the integers after validating them; filters stay bound with `?`. Count with `/*+ MAX_EXECUTION_TIME(5000) */`.
9. **Explain**: `EXPLAIN FORMAT=JSON <stmt>`.
10. Detect MariaDB from `SELECT VERSION()` and skip MySQL-only syntax there (MariaDB uses `max_statement_time` in seconds instead of `MAX_EXECUTION_TIME`).

**Tests.** conformance kit against `TEST_MYSQL_URL` (MySQL 8.4) and, if set, `TEST_MARIADB_URL`; a test that a table named with a backtick works in structure and paging.

**Gate.**

```bash
docker compose -f docker/compose.yml up -d mysql mariadb
TEST_MYSQL_URL=mysql://studio:studio@127.0.0.1:33069/studio \
TEST_MARIADB_URL=mysql://studio:studio@127.0.0.1:33079/studio cargo test -p driver-mysql
cargo clippy -p driver-mysql -- -D warnings
```

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 5 of docs/BUILD_GUIDE.md section 7.
Implement crates/driver-mysql with mysql_async (rustls): TLS modes, databases as schemas,
streaming query_iter in batches, type decoding (temporal values as server text),
read-only transactions with MAX_EXECUTION_TIME (max_statement_time on MariaDB), KILL
QUERY cancel on a second connection, metadata ported from the Electron app with
backtick escaping everywhere, paging/count and EXPLAIN FORMAT=JSON.
It must pass the conformance kit on MySQL 8.4 and MariaDB.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 6 - SQL safety (`studio-safety`)

**Goal.** A pure, heavily tested crate that decides, for every statement, whether it is a read, whether the AI may run it unattended, how risky it is, and that issues and checks approval tokens.

**Files.** `crates/studio-safety/src/{lib.rs, dialect.rs, lexer.rs, split.rs, classify.rs, functions.rs, risk.rs, approval.rs}`, `crates/studio-safety/tests/{cases_postgres.rs, cases_mysql.rs, cases_sqlite.rs, approvals.rs, props.rs}`.

**Crates.** `sqlparser` (feature `visitor`), `sha2`, `rand`, `hex`, `dashmap`, `proptest` (dev).

**Tasks.**

1. **Lexer pre-pass** (`lexer.rs`): find MySQL executable comments `/*! ... */` and `/*M! ... */` outside strings. Their presence makes a statement non-read and non-AI-auto.
2. **Split** (`split.rs`): parse the whole script with the engine's dialect and return each statement with its byte span in the original text, so the UI can highlight it. If parsing fails, fall back to a conservative splitter (semicolons outside quotes, comments and dollar quotes) and mark every piece `parse_failed`.
3. **Classify** (`classify.rs`): walk each statement's AST and fill `Verdict { kind, read_only, ai_auto, risk, reason }` using the rules table in the architecture document:
   1. SELECT/SHOW/DESCRIBE/plain EXPLAIN = read, ai\_auto.
   2. SELECT INTO = write. SELECT FOR UPDATE/SHARE = read, not ai\_auto.
   3. A WITH whose body or any CTE is INSERT/UPDATE/DELETE/MERGE = write.
   4. EXPLAIN ANALYZE = classified as its inner statement.
   5. Any call to a function on the deny-list (`functions.rs`, per dialect, matched case-insensitively and with or without schema prefix) = read but not ai\_auto.
   6. DML and DDL = write; GRANT/REVOKE/SET ROLE = write; BEGIN/COMMIT/ROLLBACK/SAVEPOINT = `Tcl`, never ai\_auto.
   7. Parse failure = write, not ai\_auto, reason "could not parse".
4. **Risk** (`risk.rs`): High for DROP, TRUNCATE, DELETE or UPDATE without WHERE, ALTER ... DROP, GRANT/REVOKE; each with a plain-language reason (port `assessRisk` from the Electron app).
5. **AI gate**: `ai_auto_allowed(sql, dialect) -> Result<(), String>` requires exactly one statement and `ai_auto = true`.
6. **Approvals** (`approval.rs`): `ApprovalBook` with `issue(session, sql, origin) -> ApprovalToken` and `consume(token, session, sql) -> Result<Origin, SafetyError>`. Token = 32 random bytes (OS RNG), hex-encoded; the book stores only `sha256(token)`; entry holds session id, `sha256(sql)`, expiry (60 s), origin (Editor, Grid, Import, Ai). `consume` removes the entry first, then checks expiry, session and SQL hash. A background sweep (or a check on each call) drops expired entries.

**Tests.**

- Case tables with at least 150 rows per dialect: port every case from the Electron `readonly.test.mjs` and `classifier.test.mjs`, then add: strings and comments containing keywords, dollar-quoted bodies, `SELECT 1; DROP TABLE x`, data-modifying CTEs, nested EXPLAIN ANALYZE, schema-qualified deny-list functions (`pg_catalog.pg_sleep`), quoted identifiers named like keywords, executable comments, empty input, very long input.
- Approvals: reuse fails; expired fails; other SQL fails (one character changed); other session fails; mismatch still burns the token.
- `proptest`: random strings never panic the classifier; any string containing an executable comment is never ai\_auto.

**Gate.** `cargo test -p studio-safety && cargo clippy -p studio-safety -- -D warnings`

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 6 of docs/BUILD_GUIDE.md section 7, and the
SQL safety chapter of the architecture document if present in docs/.
Implement crates/studio-safety: executable-comment lexer, sqlparser-based splitter with
spans and a conservative fallback, AST classifier following the rules exactly (fail
closed on parse errors), per-dialect side-effect function deny-list, risk reasons,
ai_auto_allowed, and ApprovalBook (hash-only storage, 60 s, single use, consume removes
before checking). Use the sqlparser `visitor` feature; check its API on docs.rs for the
version in Cargo.lock. Write the case tables (150+ rows per dialect), approval tests
and proptest properties. This crate must have no I/O.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 7 - Session manager (`studio-session`)

**Goal.** The single place where SQL meets a database: open sessions, apply profile policy and safety verdicts, require approvals, stream results, cancel runs and manage manual transactions.

**Files.** `crates/studio-session/src/{lib.rs, manager.rs, open.rs, run.rs, policy.rs, tx.rs, audit.rs}`, `crates/studio-session/tests/{policy.rs, approvals.rs, stream.rs}`.

**Tasks.**

1. `SessionManager::new(drivers: HashMap<Engine, Arc<dyn Driver>>, approvals: Arc<ApprovalBook>, audit: Arc<dyn AuditSink>)`.
2. `connect(profile, password, ssh_secret) -> SessionInfo`: (SSH tunnel hook stubbed until Phase 14), build `ConnectConfig`, connect, store `OpenSession { profile, session, tunnel, policy, tx_state }`.
3. `AuditSink` trait defined here (so this crate does not depend on studio-store): `async fn record(&self, entry: AuditEntry)`. The app implements it with the store.
4. `prepare(session_id, sql) -> Vec<StatementVerdict>`: split + classify, used by the UI before running.
5. `run_user(session_id, sql, approval: Option<ApprovalToken>, run_id, sink)`:
   1. classify every statement;
   2. if the profile is read-only and any statement is not a read, refuse with `ReadOnlyProfile` (even with a token);
   3. if any statement is not a read, require `approval` and `consume` it against the whole script text; one approval covers the reviewed script, once;
   4. run statements in order on one connection (the pinned one in manual mode), streaming each result's batches to `sink` tagged with the statement index; stop at the first error;
   5. write an audit entry for each non-read statement with outcome and rows affected.
6. `run_ai_read(session_id, sql, query_id, sink)`: `ai_auto_allowed` must pass; then `ExecOptions { read_only_tx: true, timeout: 30 s, max_rows: 5000 }`. There is no approval parameter on purpose.
7. `run_ai_approved(session_id, sql, query_id, token, sink)`: consume the token (origin must be `Ai`), respect read-only profiles, timeout 60 s, max\_rows 5000, audit with origin `Ai`.
8. `cancel(run_id)`: cancel the token and call the driver's canceller; a cancelled run returns `Cancelled`, never a partial success.
9. Manual transactions (`tx.rs`): `set_tx_mode(session_id, Auto | Manual)`, `commit`, `rollback`, `tx_state` (with a dirty flag set after any write). Disconnecting with a dirty transaction returns an error unless `force = true`, which rolls back.
10. Delegations: `list_schemas`, `list_objects`, `columns`, `table_structure`, `table_page`, `table_count`, `explain`.
11. A concurrency rule: one running statement per session at a time in manual mode (the pinned connection); in auto mode, runs use pool connections and may overlap.

**Tests.** with a fake in-memory `Driver` (no Docker needed): read runs without a token; write without token returns `ApprovalRequired`; token for other SQL fails; read-only profile refuses a write with a valid token; `run_ai_read` refuses `UPDATE` and runs `SELECT` with read\_only\_tx set; cancel works; manual mode commit/rollback calls reach the driver; audit entries recorded only for writes. Then one integration test per real driver behind the env vars.

**Gate.** `cargo test -p studio-session && cargo clippy -p studio-session -- -D warnings`

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 7 of docs/BUILD_GUIDE.md section 7.
Implement crates/studio-session: SessionManager with connect, prepare, run_user,
run_ai_read (no approval parameter), run_ai_approved, cancel, manual transactions and
delegated metadata calls, following the numbered rules exactly. Define the AuditSink
trait here. Write the tests against a fake in-memory Driver first, then add the
real-driver integration tests gated by TEST_PG_URL / TEST_MYSQL_URL.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 8 - Tauri app layer (`studio-app`)

**Goal.** The desktop app wires the crates together, exposes typed commands, generates TypeScript bindings, and locks the webview down with capabilities and a CSP.

**Files.** `src-tauri/src/{main.rs, lib.rs, state.rs, error.rs, setup.rs, logging.rs, audit_sink.rs}`, `src-tauri/src/commands/{mod.rs, profiles.rs, secrets.rs, sessions.rs, schema.rs, query.rs, history.rs, drafts.rs, settings.rs}`, `src-tauri/capabilities/main.json`, `src-tauri/tauri.conf.json`, `src-tauri/build.rs`.

**Crates.** `tauri` 2, `tauri-build`, `tauri-specta`, `specta`, `specta-typescript` (pin the exact versions the tauri-specta README asks for), `tauri-plugin-dialog`, `tauri-plugin-opener`, `tauri-plugin-process`, `tracing-subscriber`, `tracing-appender`.

**Tasks.**

1. **Setup** (`setup.rs`): resolve the app data dir and log dir via Tauri's path API; init `tracing` to a daily-rotated file plus stderr in debug; install the rustls crypto provider; open `Store`; choose `KeyringSecretStore` or fall back to `MemorySecretStore` with a warning event; build `SessionManager` with the Postgres and MySQL drivers; manage `AppState`.
2. **Commands as thin wrappers.** Each `#[tauri::command] #[specta::specta] async fn` validates input, calls a plain function `fn x(state: &AppState, ...)` that holds the logic, and maps errors to `AppError`. Unit-test the plain functions directly.
3. Commands for this phase: `list_profiles`, `save_profile`, `delete_profile`, `set_secret`, `clear_secret`, `test_connection`, `connect`, `disconnect`, `list_schemas`, `list_objects`, `columns`, `table_structure`, `table_page`, `table_count`, `prepare_query`, `approve_query`, `run_query` (with `Channel<RunEvent>`), `cancel_run`, `set_tx_mode`, `commit_tx`, `rollback_tx`, `explain_query`, `history_list`, `history_clear`, `drafts_list`, `drafts_save`, `drafts_delete`, `settings_get`, `settings_set`.
4. `approve_query(session_id, sql)` is the only editor-side caller of `ApprovalBook::issue`. It re-runs `prepare` and returns the token plus verdicts. The UI calls it only from the confirm dialog's button.
5. `RunEvent` channel messages: `StatementStart { index, span, columns }`, `Rows { index, batch }`, `StatementDone { index, outcome }`, `Finished`.
6. **Bindings**: build a `tauri_specta::Builder` with `collect_commands![...]` and `collect_events![...]`; in debug builds export to `../src/bindings.ts`; add a test `export_bindings` that exports so CI can check the file is current (`git diff --exit-code src/bindings.ts`). Call `builder.mount_events(app)` in setup.
7. **Capabilities** (`capabilities/main.json`, appendix A.3): only `core:default`, dialog open/save, opener for URLs, process restart, updater (added in Phase 16). Install no shell or fs plugin.
8. **CSP** in `tauri.conf.json` (appendix A.2).
9. **Audit sink**: implement `AuditSink` over `Store`.
10. Window: 1280x840, min 960x640, title "AI DB Studio".

**Tests.** unit tests for the plain functions with a temp store, memory secrets and a fake driver: saving a profile with a password stores the secret and `list_profiles` shows `has_password: true` and no password; `approve_query` then `run_query` succeeds; reusing the token fails; `export_bindings` produces a file that `tsc` compiles.

**Gate.**

```bash
cargo test -p studio-app
cargo test -p studio-app export_bindings && git diff --exit-code src/bindings.ts
npm run typecheck
cargo tauri dev     # app starts, no errors in the terminal
```

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 8 of docs/BUILD_GUIDE.md section 7, plus
appendix A.2 and A.3. Implement src-tauri (studio-app): setup (paths, tracing, rustls
provider, store, secret store with fallback, session manager), AppState, AppError
mapping, the listed commands as thin wrappers over testable plain functions, the
RunEvent channel, approve_query as the only editor-side token issuer, tauri-specta
bindings export (with an export_bindings test), capabilities and CSP. Check the
tauri-specta README for the exact specta versions and builder API.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 9 - Frontend shell, connections and explorer

**Goal.** The app looks and feels like the Electron version: title bar, activity bar, side panel with the schema explorer, tabs, status bar, themes, command palette and the connection manager, all on the new Rust API.

**Source to reuse.** Copy from the Electron repo `src/renderer/components/{ui, shell, Connections, Settings, Modals}`, `src/renderer/context/{StudioContext, SchemaContext, WorkspaceContext}.tsx`, `src/renderer/lib/{utils, sql, grid, monaco}.ts`, `index.css` and `tailwind.config.js`. Leave the AI components for Phase 13.

**Files (new).** `src/api/{index.ts, profiles.ts, sessions.ts, schema.ts, query.ts, history.ts, settings.ts, errors.ts}`, `src/main.tsx`, `src/App.tsx`.

**Tasks.**

1. **API adapter.** `src/api/*` wraps `commands` from `bindings.ts`, converts the generated `Result` shape into thrown `ApiError` objects with `code`, and exposes the same method names the contexts used (`getProfiles`, `connect`, ...) so the port is mostly mechanical.
2. Replace every `window.electronAPI.*` call with the adapter. Delete `preload`-related types.
3. Theme: apply the cached theme before first paint (as the Electron `main.tsx` does). Window controls come from Tauri's native decorations; keep a custom title bar only if you also implement dragging with `data-tauri-drag-region`.
4. Native menu replacement: keyboard shortcuts handled in the frontend (`Cmd/Ctrl+K`, `+B`, `+J`, `+L`, `+T`, `+W`, `+S`, `+,`, `+1..4`). Prevent the webview's default for `Cmd/Ctrl+W` and `Cmd/Ctrl+R` in production.
5. **Connection dialog** with tabs:
   1. General: name, engine, host, port (default per engine), database, username, password (write-only field: shows "saved" with a Replace button when `has_password`), environment, read-only switch, colour.
   2. TLS: mode select (Disable, Require, Verify CA, Verify full; default Verify full), CA file, client certificate and key files chosen with the dialog plugin.
   3. SSH: placeholder tab showing "available in a later version" until Phase 14.
   4. Test connection button showing server version and whether TLS was verified.
6. Connection states in the explorer (offline, connecting, connected, failed) with errors from `AppError.message`.
7. **Explorer tree**: connections, then schemas (databases for MySQL), then tables, views and materialized views, then columns, keys, indexes; lazy loading per level; search; keyboard navigation (port from `DatabaseExplorer.tsx`).
8. Environment band: a coloured strip in the title bar for Staging (amber) and Production (red) and a READ-ONLY badge, based on the active tab's connection.
9. TanStack Query for server state (profiles, schemas, objects, structures), invalidated after DDL and on reconnect.

**Tests.** Vitest + Testing Library: the API adapter maps errors; connection dialog never renders a stored password; explorer renders a mocked schema and filters by search.

**Gate.**

```bash
npm run typecheck && npm test
cargo tauri dev
# Manual: add a PostgreSQL and a MySQL profile (docker containers), test, connect,
# browse schemas/tables/columns, switch theme, restart the app: profiles persist,
# password shows as "saved" and is never displayed.
```

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 9 of docs/BUILD_GUIDE.md section 7.
The Electron source is at <PATH TO OLD REPO>. Port the shell, ui, Connections, Settings
and Modals components plus Studio/Schema/Workspace contexts and lib helpers into src/.
Create src/api/* wrapping the generated bindings and replace every window.electronAPI
call. Build the connection dialog with General and TLS tabs (password write-only),
the explorer with lazy loading, environment band and READ-ONLY badge, and keyboard
shortcuts. Use TanStack Query for server state. Add the listed Vitest tests.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 10 - SQL editor, run flow, streaming results, history

**Goal.** Write SQL with completion, run it with the review-before-write flow, and see results stream into a virtual grid that stays smooth at a million rows.

**Files.** port `Workbench/{SqlEditor, SqlTab, QueryResultsGrid, ResultTab, ConnectionSelect}.tsx`, `shell/BottomPanel.tsx`, `Modals/{WarningConfirmModal, DangerConfirmModal}.tsx`; new `src/lib/resultStore.ts`, `src/api/run.ts`.

**Tasks.**

1. Monaco setup as before (themes, SQL language, workers through Vite). Completion source from SchemaContext: tables, columns, aliases.
2. **Run flow** (`src/api/run.ts`):
   1. `prepareQuery(session, sql)` returns per-statement verdicts with spans and risk reasons;
   2. all reads: call `runQuery(session, sql, null, runId, channel)` directly;
   3. any write: open the confirm dialog listing each write statement, its kind and the target connection and environment; High risk uses the danger dialog with the "I understand this cannot be undone" checkbox; Production profiles also require typing the connection name;
   4. on Confirm only: `approveQuery(session, sql)` then `runQuery` with the returned token;
   5. Cancel shows a log line "Statement not run: confirmation declined".
3. **Streaming store** (`resultStore.ts`): per run, per statement: columns, a growing array of row chunks, status, elapsed time, truncated flag. Append each `Rows` batch without copying earlier rows. Grid reads through TanStack Virtual.
4. Grid features from the Electron version: column resize, client-side sort and filter on loaded rows, selection, keyboard navigation, copy as TSV/CSV/JSON. Show "Fetching... N rows" with a Stop button while streaming.
5. Multiple statements: one result tab per statement that returned rows; command results as log lines.
6. Error display: the failing statement's span is underlined in the editor using `detail.position` when the database gives one.
7. Cancel button and `Cmd/Ctrl+.` call `cancelRun(runId)`.
8. History panel and saved queries (drafts) with autosave every 2 s; unsaved marker on tabs.
9. Explain button shows the JSON plan as text for now (visual viewer in Phase 15).
10. Schema reload after successful DDL.

**Tests.** resultStore appends 1 000 batches of 500 rows in under 200 ms in jsdom; run flow: reads never open the dialog, writes always do, cancel never calls `approveQuery`; danger dialog requires the checkbox.

**Gate.**

```bash
npm run typecheck && npm test
cargo tauri dev
# Manual: SELECT from generate_series(1, 1000000) streams without freezing; Stop works;
# INSERT opens the confirm dialog; DROP TABLE opens the danger dialog; a declined write
# does not run; history lists runs; drafts survive restart.
```

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 10 of docs/BUILD_GUIDE.md section 7.
Port the SQL editor, results grid, result tabs, bottom panel and confirm dialogs.
Implement src/api/run.ts with the prepare -> (confirm -> approve) -> run flow exactly as
listed, the streaming resultStore fed by the RunEvent channel, multi-statement result
tabs, error underlining, cancel, history and autosaved drafts. The frontend must never
decide safety itself: it only displays verdicts from prepareQuery.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 11 - Table data, structure and ER diagram

**Goal.** Browse any table with server-side paging, sorting and filtering, edit rows through reviewed SQL, and inspect structure and relationships.

**Files.** port `Workbench/{TableDataTab, TableStructureView, ErDiagramView}.tsx`, `Modals/{EditCellModal, InsertRowModal, AdvancedFilterModal, CreateTableModal}.tsx`; new Rust command module `src-tauri/src/commands/rows.rs`.

**Tasks.**

1. Table data tab uses `table_page` and `table_count` (count shows "of many" on timeout).
2. **Row edits are built in Rust**, not in the frontend: new command `plan_row_change(session, table, change)` where change is Insert { values }, Update { key, values } or Delete { keys }. It quotes identifiers per dialect, uses all primary-key columns (or every column when there is no key, with a warning), renders literals safely, and returns the SQL text. The frontend then runs it through the normal Phase 10 confirm flow, so edits are reviewed and audited like typed SQL.
3. Multi-row delete combines keys with OR inside one statement.
4. Structure view: columns, indexes, foreign keys, constraints, size, owner, row estimate, DDL with copy button.
5. ER diagram per schema from foreign keys (port the existing SVG layout).
6. Create-table dialog generates DDL in Rust (`plan_create_table`) and runs through the confirm flow.

**Tests.** Rust snapshot tests for `plan_row_change` on PostgreSQL, MySQL and SQLite: composite keys, NULL keys (`IS NULL`), quotes in values, identifiers needing quoting, no-key fallback. Frontend: editing a cell opens the confirm dialog with the planned SQL.

**Gate.** `cargo test -p studio-app rows && npm run typecheck && npm test`, then manually edit, insert and delete rows in both engines.

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 11 of docs/BUILD_GUIDE.md section 7.
Port the table data, structure and ER views and the row/filter/create-table modals.
Add Rust commands plan_row_change and plan_create_table that generate dialect-correct
SQL (full primary key or all-column fallback, safe literals, quoted identifiers) and
route every edit through the existing prepare/confirm/approve/run flow. Add insta
snapshot tests for all three dialects.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 12 - AI backend (`studio-ai`)

**Goal.** An agent that answers questions about the connected database, runs safe reads by itself, pauses every other statement for a human, and never leaks rows to the model or to memory unless the user allows it.

**Files.** `crates/studio-ai/src/{lib.rs, provider.rs, providers/{openai.rs, ollama.rs, fake.rs}, prompt.rs, tools.rs, agent.rs, limits.rs, describe.rs, memory.rs, offline.rs, hub.rs, mask.rs}`, `crates/studio-ai/tests/agent.rs`; commands in `src-tauri/src/commands/ai.rs`.

**Crates.** `rig-core` (check its current provider and streaming API on docs.rs), `schemars` or `serde_json` for tool schemas, `tokio-util`, `futures`.

**Tasks.**

1. **Provider trait** (`provider.rs`), our own small interface so the agent does not depend on rig directly:

   ```rust
   #[async_trait::async_trait]
   pub trait Provider: Send + Sync {
       fn supports_tools(&self) -> bool;
       async fn stream(&self, req: ChatRequest, cancel: CancellationToken)
           -> Result<BoxStream<'static, Result<ChatChunk, AiError>>, AiError>;
   }
   pub enum ChatChunk { Text(String), ToolCall { id: String, name: String, args_json: String }, Done }
   ```
2. Adapters `openai.rs` (any OpenAI-compatible base URL: OpenAI, OpenRouter, LM Studio, vLLM) and `ollama.rs`, built on rig-core. `fake.rs` replays a scripted list of chunks for tests.
3. **System prompt** (`prompt.rs`): port `buildSystemPrompt` from the Electron `agent.ts`: connection name and dialect, behaviour rules, how to run SQL (tools or fenced blocks), what the model will receive about results, the earlier-conversation summary, the schema truncated at 30 000 characters.
4. **Tools** (`tools.rs`): `run_sql { sql }`, `describe_table { schema?, table }`, `explain_query { sql }`, `list_tables { schema? }` with JSON schemas.
5. **Loop** (`agent.rs`): up to 8 model steps; tool calls are executed in order and their outputs appended as tool messages; text chunks stream to the UI as `AiEvent::Text`. If the provider rejects tools (detect from the error), switch to fenced-SQL mode for this turn, remember the model as no-tools, and restart the step.
6. **`handle_run_sql`**: exactly the review-gate code in the architecture document: limits (no duplicate statement, at most 6 per turn), `ai_auto_allowed`, `QueryStart` event, a `oneshot` stored in the hub when review is needed, wait with `tokio::select!` on cancel, then `run_ai_read` or (after approval) `run_ai_approved` with a token minted by the `ai_review` command.
7. **Describe results for the model** (`describe.rs`): shape only (row count, column names) by default; when sharing is on, at most 50 rows and 8 000 characters as JSON, wrapped in text that says the data is untrusted. Apply masking rules (`mask.rs`) to shared rows: masked columns become `***`.
8. **Per-profile AI policy**: `Disabled` refuses to start a turn; `LocalOnly` refuses non-local endpoints (anything not localhost or 127.0.0.1); `NoRowSharing` forces sharing off.
9. **Memory** (`memory.rs`) on studio-store's conversations/turns tables: store user text, assistant text and a list of statements with outcomes, never rows. Compaction after the reply when stored text exceeds 24 000 characters: summarise all but the last 6 turns with the model (instructed to omit data values); replace only if the summary is non-empty.
10. **Offline engine** (`offline.rs`): port `AiService.heuristicSql` (count, latest, top N, columns mentioned, list tables) and run its SQL through the same loop with a static provider when the model is unreachable and nothing has streamed yet.
11. **Hub** (`hub.rs`): active turns by id with their cancel token and pending review senders; `review(turn, query, approved)`, `cancel(turn)`, `cancel_query(turn, query)`. Ending or cancelling a turn resolves all pending reviews as declined.
12. **Commands**: `ai_send(request, Channel<AiEvent>)`, `ai_review(turn_id, query_id, approved)` (the only AI-side caller of `ApprovalBook::issue`, origin `Ai`, bound to the pending statement's exact SQL), `ai_cancel`, `ai_cancel_query`, `ai_forget(conversation, from_turn?)`, `ai_test_provider`.
13. API keys are read from the secret store per provider; never placed in prompts, events or logs.

**Tests** (port every case from the Electron `agent.test.mjs`, using the fake provider and a fake session):

- a safe read runs with no review; a write waits and does not run when declined; runs only after approval;
- nothing the model sends can skip review (tool args with extra fields, fenced SQL in no-tools mode, multi-statement scripts);
- with auto-run off, even a read waits and still runs read-only;
- rows reach the model only when sharing is on, capped at 50 rows / 8 000 chars, masked columns hidden;
- rows never enter memory, even with sharing on;
- duplicate statement not re-run; 7th query refused;
- a failed query's error goes back to the model;
- without a connection nothing can run; `Disabled` policy refuses the turn; `LocalOnly` refuses a cloud endpoint;
- compaction keeps the last 6 turns and loses nothing when summarising fails.

**Gate.** `cargo test -p studio-ai && cargo test -p studio-app ai && cargo clippy --workspace -- -D warnings`

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 12 of docs/BUILD_GUIDE.md section 7.
The Electron AI code is at <PATH TO OLD REPO>/src/main/services/ai and AiService.ts.
Implement crates/studio-ai: our own Provider trait with OpenAI-compatible and Ollama
adapters on rig-core plus a scripted fake provider, the system prompt, four tools, the
step loop with no-tools fallback, handle_run_sql with the oneshot review gate, result
description with sharing caps and masking, per-profile AI policy, memory with
compaction (never rows), the offline engine, and the hub. Add the ai_* commands; ai_review
is the only AI-side token issuer. Port every agent.test.mjs case as a Rust test.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 13 - AI frontend

**Goal.** The chat experience from the Electron version: a full AI tab and a side panel sharing conversations, streaming replies, review cards, result tables and charts, and AI settings.

**Files.** port `components/ai/{AiChatView, ChatComposer, MessageContent, SqlBlock, ResultChart}.tsx`, `shell/AiPanel.tsx`, `context/ChatContext.tsx`, `lib/{chart, markdown}.ts`; new `src/api/ai.ts`.

**Tasks.**

1. `ChatContext` creates one `Channel<AiEvent>` per turn and applies events to that turn's message (no global event filtering).
2. Conversations list, rename, delete (calls `ai_forget`), regenerate (forget from that turn, then resend).
3. **Review card** for `QueryStart { needs_review: true }`: shows the SQL (highlighted), target connection, environment band, why it needs review (`reason`), risk reasons; Approve and Decline buttons; High risk requires the checkbox; Production requires typing the connection name. Approve calls `ai_review(..., true)`.
4. Running and finished query parts: status, duration, row count; result table (virtual grid, kept in memory only); auto chart (`chooseChart`: stat, line, bar) with a toggle to table.
5. Composer: send, stop (`ai_cancel`), per-query cancel, model menu with "Run safe reads automatically" and "Let the model read query results" toggles (sharing disabled and explained when the profile policy forbids it).
6. **AI settings** page: provider (Ollama, OpenAI-compatible), endpoint, model, API key (write-only, stored via `set_secret`), Test button (`ai_test_provider`), per-profile AI policy, masking rules editor (schema, table, column).
7. Markdown rendering with `react-markdown` + `remark-gfm`; links open only `https:` and `mailto:` through the opener plugin.
8. Conversations shown in the UI are kept in browser storage without result rows; the source of truth for memory is the Rust store.

**Tests.** event reducer applies a scripted event sequence correctly; review card blocks Approve until the checkbox for High risk; links with `javascript:` or `file:` are not clickable; no result rows are written to browser storage.

**Gate.** `npm run typecheck && npm test`, then manually with Ollama (`ollama pull llama3.1`): ask "how many rows are in each table", see auto-run reads and a chart; ask "delete user 5", see a review card, decline, confirm nothing changed, then approve and confirm the audit log entry.

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 13 of docs/BUILD_GUIDE.md section 7.
Port the AI chat view, composer, message rendering, SQL block/review card, charts,
AI side panel and ChatContext from <PATH TO OLD REPO>. Use one Channel<AiEvent> per
turn. Build review cards with risk and production safeguards, the model menu toggles
respecting profile policy, and the AI settings page with provider test, write-only API
key, per-profile policy and masking rules. Add the listed Vitest tests.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 14 - SSH tunnels and certificate management (`studio-tunnel`)

**Goal.** Connect to databases behind a bastion host with verified host keys, and manage CA and client certificates from the UI.

**Files.** `crates/studio-tunnel/src/{lib.rs, client.rs, forward.rs, known_hosts.rs, keys.rs}`, `crates/studio-tunnel/tests/tunnel.rs`; updates to `studio-session/src/open.rs`, the connection dialog SSH tab and `src-tauri/src/commands/ssh.rs`.

**Crates.** `russh` (crypto feature `aws-lc-rs`, or `ring` if Windows builds fail), `russh` key loading (check whether keys live in `russh::keys` or a separate crate for the version you use), `sha2`, `base64`.

**Tasks.**

1. `Tunnel::open(cfg: TunnelConfig, host_keys: &dyn HostKeyStore) -> Result<Tunnel, TunnelError>`: connect to the bastion, check the host key, authenticate (password, private key with optional passphrase, or SSH agent), bind `127.0.0.1:0`, and for each accepted socket open a `direct-tcpip` channel to the database host and port, copying both ways with `tokio::io::copy_bidirectional`.
2. **Host keys** (`known_hosts.rs`): `HostKeyStore` trait backed by studio-store's `known_hosts` table. Unknown key: return `TunnelError::HostKeyUnknown { host, port, key_type, fingerprint_sha256 }` without connecting. Changed key: `HostKeyChanged` (hard error; the UI shows both fingerprints and a warning, and only an explicit "replace" action updates it).
3. Command `trust_host_key(profile_id, fingerprint)`; the connect flow in the UI: try connect, get `HostKeyUnknown`, show the fingerprint, user trusts, connect again.
4. `Tunnel` implements `Drop` (cancels its task and closes the listener); keep-alive every 30 s; a dropped SSH session emits a global `connection-lost` event with the session id.
5. Session manager: when the profile has `ssh`, open the tunnel first and point the driver at `127.0.0.1:<local_port>`, keeping the original host name for TLS verification (`server_name` = database host, not 127.0.0.1).
6. UI SSH tab: host, port, user, auth method, key file picker, passphrase/password (write-only), Test SSH button.
7. Certificate UX: CA file and client cert/key pickers already exist from Phase 9; add a "Show server certificate" action that returns the presented chain's subjects, issuers, validity dates and SHA-256 fingerprints from `test_connection`.

**Tests.** with the `ssh` container from appendix A.4 (an OpenSSH server on port 2222 that can reach the `postgres` service): tunnel + query works with password and with key auth; first connect returns `HostKeyUnknown`; after trust it connects; a different stored key returns `HostKeyChanged`; dropping the tunnel frees the port.

**Gate.** `docker compose -f docker/compose.yml up -d ssh postgres && TEST_SSH=1 cargo test -p studio-tunnel -p studio-session`, then connect through SSH from the app.

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 14 of docs/BUILD_GUIDE.md section 7.
Implement crates/studio-tunnel with russh: Tunnel::open with password/key/agent auth,
host-key checking through a HostKeyStore backed by studio-store (unknown = error with
fingerprint, changed = hard error), local port forwarding with copy_bidirectional,
Drop-based shutdown and keep-alive. Wire it into the session manager keeping the real
host name for TLS, add trust_host_key, the SSH tab, and server certificate display.
Verify the russh API for the version in Cargo.lock. Tests use the ssh container.
Plan first, wait for approval, then implement and run the gate.
```

---

### Phase 15 - Production features

**Goal.** The features that make it safe and complete for daily production work. Split this phase into sub-sessions 15a to 15g; each has its own gate.

| Sub | Feature | Key tasks | Gate |
| --- | --- | --- | --- |
| 15a | Manual transactions UI | Auto/Manual toggle per tab connection; pending-changes badge; Commit and Rollback buttons; prompt on tab close or disconnect with a dirty transaction | commit and rollback visibly work in both engines |
| 15b | Production and read-only profiles | Production requires typing the connection name for every write; read-only rejects writes with a clear message; AI policy defaults for production: no row sharing | session tests + manual check |
| 15c | Audit log viewer | Table with filters (profile, origin, kind, date); export CSV/JSON; "Verify integrity" button calling `verify_chain` | tampering a row in a copy of the DB is detected |
| 15d | Export | `export_query` and `export_table` stream rows from the driver straight to a file chosen with the save dialog: CSV (`csv` crate), JSON lines, XLSX (`rust_xlsxwriter`), SQL INSERT statements; progress events; cancel | 1 M-row export finishes with flat memory use |
| 15e | CSV import | Wizard: choose file, map columns, preview 100 rows, choose batch size; Rust builds multi-row INSERTs in one transaction; one approval covers the import; audit entry with row count | import 100 k rows; a bad row rolls back everything |
| 15f | Visual EXPLAIN | Parse `EXPLAIN (FORMAT JSON)` / `FORMAT=JSON` into a tree; show node type, relation, cost, rows; highlight the most expensive nodes; "Ask AI to explain" button | plans render for both engines |
| 15g | SQLite driver + data masking in the grid | `driver-sqlite` with `rusqlite` (bundled) on blocking threads, read-only open for AI reads, interrupt handle for cancel, passes the conformance kit; optional masking of tagged columns in the grid | `cargo test -p driver-sqlite`; masked column shows `***` |

Optional 15h: encrypted app database. Build `studio-store` with SQLCipher (`libsqlite3-sys` feature `bundled-sqlcipher`), keep a random 256-bit key in the keychain (`SecretKey::StoreKey`), and add a Settings switch that migrates the file. Ship only after testing migration both ways.

**Prompt (repeat per sub-phase).**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 15 of docs/BUILD_GUIDE.md section 7.
We are doing sub-phase 15<x> only: <feature name>. Implement exactly the key tasks in
that table row, with tests, without touching other sub-phases.
Plan first, wait for approval, then implement and run that row's gate.
```

---

### Phase 16 - Release: data import, signing, updates, hardening

**Goal.** Users of the Electron version keep their data, and the app ships as signed, auto-updating installers for macOS, Windows and Linux.

**Tasks.**

1. **Import from the Electron version** (`src-tauri/src/commands/import_legacy.rs`): on first launch, look for the old data folder (`AI DB Studio` or `ai-db-studio` in the platform config dir). Read `studio-storage.json` and `ai-memory.json`; convert profiles (TLS `PREFER` becomes `Require` with a warning badge suggesting Verify full), drafts, history, settings and conversations into `studio.db`. Old secrets were encrypted with Electron's `safeStorage` and cannot be decrypted here: mark those profiles `has_password: false` and ask the user to re-enter passwords. Keep a backup copy of both files. Show a one-time summary dialog.
2. **Updater**: add `tauri-plugin-updater` and `tauri-plugin-process`; generate the signing key pair with `cargo tauri signer generate -w ~/.tauri/aidbstudio.key`; put the public key in `plugins.updater.pubkey` and the endpoint in `plugins.updater.endpoints`; set `bundle.createUpdaterArtifacts: true`; check on start and every 24 h; show release notes; download, verify, install, restart.
3. **Release workflow** (appendix A.7) using `tauri-apps/tauri-action`: macOS arm64 and x64 (or universal), Windows x64, Ubuntu 22.04; signed with secrets; uploads installers and `latest.json` to a draft GitHub release.
4. **macOS signing and notarisation**: Developer ID Application certificate; secrets `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID`.
5. **Windows signing**: an Authenticode certificate (OV/EV or a cloud signing service) configured in `bundle.windows` (`certificateThumbprint` or a custom `signCommand`).
6. **Linux**: AppImage, `.deb` and `.rpm` targets; test on Ubuntu 22.04 and 24.04 and Fedora.
7. **Hardening pass**: run the security checklist (section 10); `cargo deny check`; `cargo audit`; review every command for input validation; review logs for secrets and SQL text; make sure release builds have no devtools.
8. **Crash reporting (opt-in)**: a panic hook that writes a crash file; optional Sentry with SQL text, rows and secrets scrubbed.
9. **Diagnostics**: "Copy diagnostics" in Help (version, OS, webview version, last 200 log lines with secrets removed).

**Gate.** a tagged pre-release (`v1.0.0-rc.1`) builds on all three OS in CI; installers install and launch; the updater moves `rc.1` to `rc.2`; Electron data imports on a machine that had the old version; the section 10 release checklist is fully ticked.

**Prompt.**

```text
Read CLAUDE.md, docs/PROGRESS.md and Phase 16 of docs/BUILD_GUIDE.md section 7 plus
section 9 and appendices A.2 and A.7. Implement the legacy data importer, the updater
integration, the release workflow and bundle/signing configuration (secrets come from
GitHub Actions secrets, never the repo), crash file and diagnostics. Then run the
hardening pass and report every finding before fixing it.
Plan first, wait for approval, then implement step by step.
```

## 8. Testing guide

Three layers of tests run locally and in CI: fast unit tests with no services, integration tests against real databases in Docker, and a safety suite that must pass before every release.

### Local test services

```bash
docker compose -f docker/compose.yml up -d        # start everything
docker compose -f docker/compose.yml ps           # wait until all are healthy
docker compose -f docker/compose.yml down -v      # stop and delete data
```

| Service | Port | Env variable for tests | Credentials |
| --- | --- | --- | --- |
| `postgres` (17) | 54329 | `TEST_PG_URL=postgres://studio:studio@127.0.0.1:54329/studio` | studio / studio |
| `postgres-tls` (17, self-signed cert) | 54330 | `TEST_PG_TLS_URL=postgres://studio:studio@localhost:54330/studio` | studio / studio |
| `mysql` (8.4) | 33069 | `TEST_MYSQL_URL=mysql://studio:studio@127.0.0.1:33069/studio` | studio / studio |
| `mariadb` (11) | 33079 | `TEST_MARIADB_URL=mysql://studio:studio@127.0.0.1:33079/studio` | studio / studio |
| `ssh` (OpenSSH) | 2222 | `TEST_SSH=1` | tunnel / tunnel, key in `docker/ssh/test_key` |

Put the variables in a `.env.test` file (git-ignored) and load it with `set -a; source .env.test; set +a` before `cargo test`.

### Commands

| What | Command |
| --- | --- |
| All Rust tests | `cargo test --workspace` (or `cargo nextest run --workspace`) |
| One crate | `cargo test -p studio-safety` |
| One test by name | `cargo test -p studio-session read_only_profile_refuses_write` |
| Update snapshots | `cargo insta review` (install `cargo-insta`) |
| Lint | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` |
| Frontend | `npm run typecheck && npm test` |
| Bindings current | `cargo test -p studio-app export_bindings && git diff --exit-code src/bindings.ts` |
| Licences, advisories | `cargo deny check` |
| Fuzz classifier (optional) | `cargo +nightly fuzz run classify -- -max_total_time=300` |

### Rules for integration tests

- A test that needs a service reads its env variable. If it is missing, print `skipped: TEST_PG_URL not set` and return. CI always sets them, so nothing is silently skipped there.
- Each test creates its own schema or database with a random suffix and drops it at the end, so tests can run in parallel.
- Never use the real OS keychain in tests; use `MemorySecretStore` or the keyring mock.

### The safety suite (release blocker)

A dedicated test file, `crates/studio-session/tests/safety_suite.rs`, runs against every real engine:

1. Every statement in the classifier tables marked `ai_auto = true` runs through `run_ai_read`; none may change data (checked by row counts and a sentinel table before and after).
2. A list of known tricky writes (data-modifying CTE, `SELECT ... INTO`, executable comments, `EXPLAIN ANALYZE DELETE`, functions with side effects, multi-statement scripts) must be refused by `run_ai_read`, and when forced through the read-only transaction by a test-only path, must be refused by the database.
3. Approval tokens: reuse, expiry, other SQL, other session, other origin all fail.
4. Read-only profiles refuse writes with a valid token.
5. AI turns with a fake provider never put row values into memory, logs or the prompt when sharing is off.

Any failure blocks the release; fix the code, never the test.

### Manual smoke test before each release

1. Fresh install, add PostgreSQL and MySQL profiles with Verify full TLS against a real cloud database (CA file supplied).
2. Connect through SSH to a database without public access.
3. Stream 1 M rows; export them to CSV; import a 100 k-row CSV.
4. Edit, insert and delete rows; check the audit log and run Verify integrity.
5. AI: ask a read question (auto-run, chart), ask for a change (review card), decline and approve.
6. Switch a profile to Production and Read-only and confirm the guards.
7. Quit and reopen: layout, tabs, drafts and conversations restored.

## 9. Release, signing and updates

A release is a git tag `vX.Y.Z`; CI builds and signs installers for all three platforms and attaches them, plus the updater manifest, to a draft GitHub release that you publish by hand.

### One-time setup

1. **Updater keys.** `cargo tauri signer generate -w ~/.tauri/aidbstudio.key`. Store the private key and its password as GitHub secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Put the public key in `tauri.conf.json` under `plugins.updater.pubkey`. Back up the private key offline: if it is lost, installed apps can never update again.
2. **Updater endpoint.** `https://github.com/<owner>/<repo>/releases/latest/download/latest.json` in `plugins.updater.endpoints`.
3. **Apple** (Apple Developer Program): create a Developer ID Application certificate, export it as .p12, base64-encode it into `APPLE_CERTIFICATE` with its password in `APPLE_CERTIFICATE_PASSWORD`; set `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`.
4. **Windows**: buy an OV or EV code-signing certificate, or use a cloud signing service. Configure it in `bundle.windows` (thumbprint, or a `signCommand` for cloud signing). Unsigned builds work but show SmartScreen warnings.
5. **Linux**: no OS signing needed; the updater signature protects updates.

### Versioning

- Semantic versioning. The version lives in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`; keep `package.json` in step.
- Pre-releases: `v1.0.0-rc.1`. The release workflow marks them as pre-release.
- Database migrations only move forward; every release must open data created by any earlier release.

### Cutting a release

```bash
# 1. update versions and CHANGELOG.md, then
git commit -am "release: v1.2.0"
git tag v1.2.0
git push origin main --tags
# 2. wait for the Release workflow (appendix A.7) to finish on all three OS
# 3. test the draft release's installers on each OS
# 4. publish the draft release on GitHub; installed apps see the update within 24 h
```

### Update behaviour in the app

- Check on startup (after 10 s) and every 24 h; never during a running query or open manual transaction.
- Show version and release notes; the user chooses Install now or Later.
- Download, verify the signature, install, then restart with the process plugin.
- A failed or invalid update is logged and reported, never installed.

## 10. Checklists

Tick these at the end of every phase (definition of done), before merging anything that touches safety (security), and before every tag (release).

### Definition of done (every phase)

- [ ] Every task in the phase is implemented, or the gap is written in `docs/PROGRESS.md`
- [ ] New tests written for the phase, and all tests pass
- [ ] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `npm run typecheck` and `npm test` clean (frontend phases)
- [ ] `src/bindings.ts` regenerated and committed if commands changed
- [ ] No `unwrap()`/`expect()` added outside tests; no `unsafe`; no `todo!()` left
- [ ] Public items documented
- [ ] `docs/PROGRESS.md` updated (done, decisions, deviations, gaps, next)
- [ ] Diff reviewed by you, then committed as `phase N: <title>`

### Security checklist (any change to safety, secrets, IPC or AI)

- [ ] No command returns a password, passphrase, API key or connection string with a password
- [ ] No log line contains SQL parameters, row values or secrets
- [ ] Every new command validates its input in Rust
- [ ] SQL classification happens only in `studio-safety`; the frontend only displays verdicts
- [ ] `ApprovalBook::issue` is still called only from `approve_query` and `ai_review`
- [ ] AI auto-runs still go through `run_ai_read` with a READ ONLY transaction, 30 s timeout, 5 000-row cap
- [ ] Read-only profiles still refuse writes with a valid token
- [ ] Rows reach the model only with sharing on, masked columns hidden, never stored in memory
- [ ] TLS default is still Verify full; SSH host keys still pinned
- [ ] Capabilities file grants nothing new without a reason written in the PR
- [ ] CSP unchanged, or the change is justified
- [ ] Safety suite passes against PostgreSQL, MySQL and MariaDB

### Release checklist (before every tag)

- [ ] CI green on macOS, Windows and Linux
- [ ] Safety suite green
- [ ] `cargo deny check` and `cargo audit` clean, or each exception documented
- [ ] Version bumped in `tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`; `CHANGELOG.md` updated
- [ ] Migrations tested by opening a database created by the previous release
- [ ] Manual smoke test (section 8) done on each OS
- [ ] Installers signed (macOS notarised; Windows signed)
- [ ] Updater tested from the previous version to this one
- [ ] Release notes written for users, including any security-relevant change
- [ ] Private keys and certificates still only in GitHub secrets and an offline backup

## 11. Troubleshooting

The problems below are the ones most likely to stall a phase; give Claude Code the matching row when you hit one.

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Panic: "no process-level CryptoProvider available" | rustls 0.23 needs one crypto provider installed when several are compiled in | Call `rustls::crypto::aws_lc_rs::default_provider().install_default()` (or the `ring` provider) once at startup, before any TLS connection |
| Windows build fails in `aws-lc-sys` | Missing CMake/NASM or C toolchain | Install CMake and NASM, or switch rustls/russh to the `ring` backend |
| `tauri-specta` compile errors about `specta` versions | specta and tauri-specta release candidates must match exactly | Pin both to the versions in the tauri-specta README with `=` |
| Frontend call fails with a permission or "not allowed" error | A plugin API is not granted in `capabilities/main.json` | Add the exact permission (for example `dialog:allow-save`) and restart `cargo tauri dev` |
| `sqlx` macros need `DATABASE_URL` at build time | Using `query!` macros | Use runtime `query_as` with `FromRow` (this guide's default), or commit `.sqlx` with `cargo sqlx prepare` |
| Keyring errors on Linux ("no secret service") | No GNOME Keyring/KWallet running, or headless CI | Fall back to `MemorySecretStore` with a warning; in CI always use the mock store |
| Keyring calls hang or fail randomly | Concurrent access to the OS store | Route every call through the single keyring worker thread |
| Blank white window on some Linux machines | WebKitGTK GPU/DMA-BUF rendering issue | Try `WEBKIT_DISABLE_DMABUF_RENDERER=1 cargo tauri dev`; document it for affected users |
| Monaco workers fail to load | CSP blocks workers or Vite worker URLs | Import workers with Vite's `?worker` syntax and allow `worker-src 'self' blob:` in the CSP |
| PostgreSQL TLS fails with "certificate verify failed" behind an SSH tunnel | Hostname check uses 127.0.0.1 | Pass the database's real host name as the TLS server name, not the tunnel address |
| Dates shift by a day | Converting temporal values to local time | Keep `Value::Temporal` as the server's text; never parse into local `Date` in the frontend |
| MySQL `LIMIT ?` syntax error | Some servers reject bound LIMIT/OFFSET | Validate the integers and inline them; keep filter values bound |
| Query cancel does nothing on MySQL | `KILL QUERY` sent on the same busy connection | Send it on a second connection with the recorded connection id |
| Grid freezes on large results | Copying the whole row array on each batch | Append chunks; virtualise rows; never `[...rows, ...batch]` |
| `cargo tauri dev` cannot reach the dev server | Port mismatch | `server.port` in Vite and `build.devUrl` in `tauri.conf.json` must both be 1420 with `strictPort: true` |
| Tests pass locally but are skipped in CI | Missing env variables | CI must export every `TEST_*` variable from section 8 |
| Claude Code keeps changing a safety test | Treating the test as the problem | Re-state rule 11 from CLAUDE.md and ask for the root cause in the code |

If a crate API in this guide does not match what compiles, trust the compiler and docs.rs for the version in `Cargo.lock`, and record the difference in `docs/PROGRESS.md`.

## 12. Appendix: reference files

These files are starting points. Versions are the major lines current in late 2026; let Claude Code confirm exact versions with `cargo add` and the docs, and keep the structure.

### A.1 Root `Cargo.toml`

```toml
[workspace]
resolver = "2"
members = ["crates/*", "src-tauri"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
unwrap_used = "warn"
expect_used = "warn"
dbg_macro = "deny"
todo = "warn"

[workspace.dependencies]
# runtime and utilities
tokio       = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "net", "io-util"] }
tokio-util  = { version = "0.7", features = ["rt"] }
futures     = "0.3"
async-trait = "0.1"
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"
thiserror   = "2"
anyhow      = "1"
uuid        = { version = "1", features = ["v4", "serde"] }
chrono      = { version = "0.4", default-features = false, features = ["std", "clock"] }
dashmap     = "6"
secrecy     = "0.10"
sha2        = "0.10"
rand        = "0.8"
hex         = "0.4"
tracing     = "0.1"
# IPC types: use the exact versions the tauri-specta README requires
specta      = { version = "=2.0.0-rc.NN", features = ["derive", "uuid", "chrono"] }
# databases
tokio-postgres        = "0.7"
deadpool-postgres     = "0.14"
tokio-postgres-rustls = "0.13"
mysql_async           = { version = "0.36", default-features = false, features = ["minimal", "rustls-tls"] }
rusqlite              = { version = "0.37", features = ["bundled"] }
sqlx                  = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "migrate", "macros"] }
rustls                = "0.23"
rustls-native-certs   = "0.8"
rustls-pemfile        = "2"
# safety, tunnel, secrets, AI
sqlparser = { version = "0.61", features = ["visitor"] }
russh     = { version = "0.63", features = ["aws-lc-rs"] }
keyring   = { version = "3", features = ["apple-native", "windows-native", "sync-secret-service"] }
rig-core  = "0.46"
# testing
insta     = { version = "1", features = ["json"] }
proptest  = "1"
tempfile  = "3"
# internal crates
studio-types   = { path = "crates/studio-types" }
studio-db      = { path = "crates/studio-db" }
studio-safety  = { path = "crates/studio-safety" }
studio-tunnel  = { path = "crates/studio-tunnel" }
studio-session = { path = "crates/studio-session" }
studio-store   = { path = "crates/studio-store" }
studio-ai      = { path = "crates/studio-ai" }
driver-postgres = { path = "crates/driver-postgres" }
driver-mysql    = { path = "crates/driver-mysql" }
driver-sqlite   = { path = "crates/driver-sqlite" }

[profile.release]
lto = "thin"
codegen-units = 1
strip = true
panic = "abort"
```

The minor versions for `deadpool-postgres`, `tokio-postgres-rustls`, `mysql_async` and `rusqlite` move often; check them, and keep `tokio-postgres-rustls` compatible with your `rustls` and `tokio-postgres` versions. If `rusqlite` (user SQLite files) and `sqlx` (app database) disagree on `libsqlite3-sys`, align them or let `driver-sqlite` use `sqlx` too.

### A.2 `src-tauri/tauri.conf.json`

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "AI DB Studio",
  "version": "0.1.0",
  "identifier": "com.aidbstudio.app",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      { "label": "main", "title": "AI DB Studio", "width": 1280, "height": 840, "minWidth": 960, "minHeight": 640 }
    ],
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; worker-src 'self' blob:; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "createUpdaterArtifacts": true,
    "category": "DeveloperTool",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"]
  },
  "plugins": {
    "updater": {
      "pubkey": "<CONTENT OF ~/.tauri/aidbstudio.key.pub>",
      "endpoints": ["https://github.com/<owner>/<repo>/releases/latest/download/latest.json"]
    }
  }
}
```

Add the `plugins.updater` block and `createUpdaterArtifacts` only in Phase 16. Generate icons from one 1024x1024 PNG with `cargo tauri icon path/to/icon.png`.

### A.3 `src-tauri/capabilities/main.json`

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "main",
  "description": "Permissions for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "dialog:allow-open",
    "dialog:allow-save",
    "dialog:allow-message",
    "dialog:allow-ask",
    "opener:allow-open-url",
    "process:allow-restart",
    "updater:default"
  ]
}
```

Give the window the label `main` in `tauri.conf.json` (`"label": "main"`) so the capability applies. Add `updater:default` only in Phase 16.

### A.4 `docker/compose.yml`

```yaml
services:
  postgres:
    image: postgres:17
    environment: { POSTGRES_USER: studio, POSTGRES_PASSWORD: studio, POSTGRES_DB: studio }
    ports: ["54329:5432"]
    healthcheck: { test: ["CMD-SHELL", "pg_isready -U studio"], interval: 3s, retries: 20 }

  postgres-tls:
    image: postgres:17
    environment: { POSTGRES_USER: studio, POSTGRES_PASSWORD: studio, POSTGRES_DB: studio }
    ports: ["54330:5432"]
    volumes: ["./tls:/tls:ro"]      # created by docker/tls/gen.sh (CA + server cert for localhost)
    command: >
      bash -c "cp /tls/server.key /var/lib/postgresql/server.key &&
               chown postgres /var/lib/postgresql/server.key &&
               chmod 600 /var/lib/postgresql/server.key &&
               exec docker-entrypoint.sh postgres -c ssl=on
               -c ssl_cert_file=/tls/server.crt -c ssl_key_file=/var/lib/postgresql/server.key"
    healthcheck: { test: ["CMD-SHELL", "pg_isready -U studio"], interval: 3s, retries: 20 }

  mysql:
    image: mysql:8.4
    environment: { MYSQL_ROOT_PASSWORD: root, MYSQL_DATABASE: studio, MYSQL_USER: studio, MYSQL_PASSWORD: studio }
    ports: ["33069:3306"]
    healthcheck: { test: ["CMD", "mysqladmin", "ping", "-h", "127.0.0.1", "-proot"], interval: 3s, retries: 30 }

  mariadb:
    image: mariadb:11
    environment: { MARIADB_ROOT_PASSWORD: root, MARIADB_DATABASE: studio, MARIADB_USER: studio, MARIADB_PASSWORD: studio }
    ports: ["33079:3306"]
    healthcheck: { test: ["CMD", "healthcheck.sh", "--connect", "--innodb_initialized"], interval: 3s, retries: 30 }

  ssh:
    image: lscr.io/linuxserver/openssh-server:latest
    environment:
      PUID: "1000"
      PGID: "1000"
      USER_NAME: tunnel
      USER_PASSWORD: tunnel
      PASSWORD_ACCESS: "true"
      PUBLIC_KEY_FILE: /keys/test_key.pub
      DOCKER_MODS: linuxserver/mods:openssh-server-ssh-tunnel   # enables TCP forwarding
    volumes: ["./ssh:/keys:ro"]     # docker/ssh/test_key(.pub) from ssh-keygen -t ed25519 -N "" -f docker/ssh/test_key
    ports: ["2222:2222"]
```

The tunnel tests connect to `ssh` on port 2222 and forward to `postgres:5432` inside the compose network. Ask Claude Code to write `docker/tls/gen.sh` (openssl: a test CA, a server certificate for `localhost` signed by it) in Phase 4; never use these certificates or keys outside tests.

### A.5 `src-tauri/src/lib.rs` skeleton

```rust
mod commands;
mod error;
mod setup;
mod state;

use tauri_specta::{collect_commands, collect_events, Builder};

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::profiles::list_profiles,
            commands::profiles::save_profile,
            commands::profiles::delete_profile,
            commands::secrets::set_secret,
            commands::secrets::clear_secret,
            commands::sessions::test_connection,
            commands::sessions::connect,
            commands::sessions::disconnect,
            commands::query::prepare_query,
            commands::query::approve_query,
            commands::query::run_query,
            commands::query::cancel_run,
            // ... every command, grouped by module
        ])
        .events(collect_events![/* ConnectionLost, UpdateAvailable, ... */])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    builder
        .export(specta_typescript::Typescript::default(), "../src/bindings.ts")
        .expect("export TypeScript bindings"); // allowed: debug-only startup invariant

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            setup::init(app)?; // paths, tracing, rustls provider, store, secrets, sessions -> app.manage(AppState)
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running AI DB Studio");
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_bindings() {
        super::specta_builder()
            .export(specta_typescript::Typescript::default(), "../src/bindings.ts")
            .unwrap();
    }
}
```

### A.6 `.github/workflows/ci.yml`

```yaml
name: CI
on:
  push: { branches: [main] }
  pull_request:

jobs:
  lint-test:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
      - name: System deps
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libdbus-1-dev pkg-config
      - uses: dtolnay/rust-toolchain@stable
        with: { components: "clippy, rustfmt" }
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with: { node-version: 22, cache: npm }
      - run: npm ci
      - name: Test services
        run: |
          bash docker/tls/gen.sh
          ssh-keygen -t ed25519 -N "" -f docker/ssh/test_key
          docker compose -f docker/compose.yml up -d --wait
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
        env:
          TEST_PG_URL: postgres://studio:studio@127.0.0.1:54329/studio
          TEST_PG_TLS_URL: postgres://studio:studio@localhost:54330/studio
          TEST_MYSQL_URL: mysql://studio:studio@127.0.0.1:33069/studio
          TEST_MARIADB_URL: mysql://studio:studio@127.0.0.1:33079/studio
          TEST_SSH: "1"
      - run: git diff --exit-code src/bindings.ts
      - run: npm run typecheck && npm test
      - uses: EmbarkStudios/cargo-deny-action@v2

  build:
    strategy:
      fail-fast: false
      matrix: { os: [macos-latest, windows-latest, ubuntu-22.04] }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - if: startsWith(matrix.os, 'ubuntu')
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with: { node-version: 22, cache: npm }
      - run: npm ci
      - run: npm run build
      - run: cargo build --workspace --release
```

### A.7 `.github/workflows/release.yml`

```yaml
name: Release
on:
  push: { tags: ["v*"] }

permissions:
  contents: write

jobs:
  release:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { platform: macos-latest, args: "--target aarch64-apple-darwin" }
          - { platform: macos-latest, args: "--target x86_64-apple-darwin" }
          - { platform: windows-latest, args: "" }
          - { platform: ubuntu-22.04, args: "" }
    runs-on: ${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4
      - if: matrix.platform == 'ubuntu-22.04'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ startsWith(matrix.platform, 'macos') && 'aarch64-apple-darwin,x86_64-apple-darwin' || '' }}
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with: { node-version: 22, cache: npm }
      - run: npm ci
      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: "AI DB Studio ${{ github.ref_name }}"
          releaseBody: "See CHANGELOG.md for details."
          releaseDraft: true
          prerelease: ${{ contains(github.ref_name, '-rc') }}
          args: ${{ matrix.args }}
```

### A.8 `docs/PROGRESS.md` template

```markdown
# Progress

## Phase N - <title> (done YYYY-MM-DD)
- Built: <crates, commands, components>
- Decisions: <choices made and why>
- Deviations from the guide: <what and why>
- Known gaps / TODO: <items, with the phase that will close them>
- Next: Phase N+1 - <title>
```
