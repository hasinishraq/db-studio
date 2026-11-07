# AI DB Studio

A desktop client for PostgreSQL, MySQL, MariaDB and SQLite with an AI assistant that runs safe reads
by itself and changes data only after you approve the exact statement.

Tauri 2, a Rust workspace, and a React + TypeScript frontend. The plan is in `docs/BUILD_GUIDE.md`,
the current state and every deviation from the plan in `docs/PROGRESS.md`.

## Run it

Prerequisites are in section 3 of the build guide. On Windows that is the Visual Studio C++ Build
Tools and the Windows SDK.

```bash
npm ci
npx tauri dev        # the app, with reload
npx tauri build      # installers
```

## Test it

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
npm run typecheck && npm test
```

Tests that need a server read `TEST_PG_URL`, `TEST_PG_TLS_URL`, `TEST_MYSQL_URL`, `TEST_MYSQL_TLS_URL`
and `TEST_MARIADB_URL`, and print `skipped:` when one is unset. `docker compose -f docker/compose.yml up -d` starts them;
section 8 of the build guide lists the URLs. Before the TLS server starts, run `bash docker/tls/gen.sh`.

`TEST_MYSQL_TLS_URL` needs a MySQL server started with the certificates from `docker/tls`; the compose
file does not do that. End-to-end drivers for the real app are in `tests/e2e/`.

`crates/studio-session/tests/safety_suite.rs` is the release-blocking suite. It forces known writes
past the classifier and checks that each real database refuses them too.

## How writes are kept safe

1. `studio-safety` parses every statement. Only what it recognises as a read is a read, and anything
   that does not parse counts as a write.
2. A write needs a single-use token, valid for 60 seconds and bound to the exact SQL, the session and
   where the request came from. Only `approve_query` and `ai_review` mint one, and a test enforces that.
3. Unattended AI reads also run in a read-only transaction, with a timeout and a row cap, so the
   database itself is the second check.
4. Every non-read statement goes into a hash-chained audit log.
