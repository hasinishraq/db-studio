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
