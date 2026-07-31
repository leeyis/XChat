@/Users/eason/.codex/RTK.md

# AGENTS.md

## Scope

These instructions apply to the entire repository.

LANChat is a Tauri 2 application with a dependency-free HTML/CSS/JavaScript
frontend and a Rust backend. The desktop/mobile app and the headless web binary
share the SQLite, peer discovery, messaging, file transfer, and HTTP/WebSocket
code.

## Repository map

- `src/`: static frontend. `js/api.js` is the Tauri/web API boundary,
  `js/ui.js` owns rendering and interaction, and `js/app.js` boots the app.
- `src-tauri/src/main.rs`: desktop entry point.
- `src-tauri/src/lib.rs`: Tauri library/mobile entry point.
- `src-tauri/src/server_main.rs`: headless web entry point.
- `src-tauri/src/commands.rs`: Tauri commands.
- `src-tauri/src/db.rs`: SQLite initialization and persistence.
- `src-tauri/src/network/`: UDP discovery and peer messaging.
- `src-tauri/src/web_server.rs`: Axum HTTP/WebSocket server and web API.
- `src-tauri/permissions/` and `src-tauri/capabilities/`: Tauri command access.
- `src-tauri/gen/android/`: Android platform project and native integration.
- `Makefile`: release packaging targets; it is not the routine test runner.
- `plan/`: historical work notes, not the source of truth for current behavior.

## Code discovery

Prefer the codebase knowledge graph over text search:

1. `search_graph` for functions, classes, routes, and variables.
2. `trace_path` for callers, callees, impact, and data flow.
3. `get_code_snippet` for exact symbol source.
4. `query_graph` for complex relationships.
5. `get_architecture` for the high-level structure.

If the repository is not indexed, run `index_repository` for the current
repository with project name `XChat` before discovery. Fall back to `rg` for
string literals, errors, configuration, shell scripts, and other non-code
files.

## Commands

Run commands from the repository root and prefix every shell command with
`rtk`.

```bash
# Desktop development (long-running)
rtk cargo tauri dev

# Fast desktop/shared-core compile check
rtk cargo check --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features desktop --lib

# Headless web compile check
rtk cargo check --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features web --bin lanchat-web

# Existing Rust tests
rtk cargo test --manifest-path src-tauri/Cargo.toml --lib

# Release target documentation
rtk make help
```

There is no npm build, package manager, frontend framework, or JavaScript test
runner. `cargo test --lib` currently contains no tests. Add one focused test
only when non-trivial changed logic has a useful test seam.

The default development run uses port `8888` and the real platform application
data directory. For isolated runs, pass a different port and a disposable
database directory:

```bash
rtk cargo tauri dev -- --port 18888 --db-path /tmp/lanchat-agent
```

Do not run repository-wide `cargo fmt` as incidental cleanup: the current
baseline is not rustfmt-clean and doing so produces a large unrelated diff.
Keep touched Rust code rustfmt-compatible and formatting changes local.

## Change rules

- Make the smallest complete change and reuse existing modules and dependencies.
- Keep the frontend dependency-free unless the task explicitly requires a
  toolchain change.
- Put shared behavior in the existing Rust core so desktop and web paths do not
  drift.
- Gate platform APIs with the narrowest correct `cfg`; compile the affected
  feature/target when possible.
- When adding or renaming a Tauri command, update both command registrations
  (`main.rs` and `lib.rs`), `permissions/commands.toml`, and the relevant
  capability JSON.
- Preserve compatibility of serialized message models, WebSocket/HTTP payloads,
  and existing SQLite data unless a migration is explicitly part of the task.
- Avoid blocking work on async runtimes. Follow the existing `tokio::spawn` and
  state-management patterns for long-running network work.
- Do not regenerate or broadly rewrite `src-tauri/gen/android/` for an unrelated
  Rust or frontend change.
- Preserve user changes in the working tree. Do not clean, reset, commit, or
  reformat unrelated files.

## Verification

- Re-run the narrow compile/test command that exercises the changed path.
- For shared Rust changes, check both `desktop` and `web` features.
- For frontend or Tauri-command changes, launch `rtk cargo tauri dev` and smoke
  test the affected flow.
- For network, file-transfer, or database changes, use an alternate port and
  disposable database directory when isolation matters.
- Report commands run, failures, and any platform target that was not available.
