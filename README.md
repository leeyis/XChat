# Xchat

Xchat `0.1.1` is a LAN chat client built with Tauri 2, React, and Rust. Install and run the client on each device; discovery, messaging, file transfer, and SQLite storage are built in, so normal desktop use does not require a separate server.

## Features

- Automatic LAN discovery and manually configured hosts
- Direct and group chats, offline delivery, delivered/read receipts
- Four-way parallel transfer for large files, with resume, cancellation, retry, and a file center
- Pasted, selected, and dropped image drafts with inline message rendering
- Desktop capture editor (macOS, Windows, Linux) with rectangle, ellipse, arrow, pen, mosaic, text, undo, and pin
- Chinese/English UI, themes, notifications, download, network, and local IP/MAC identity settings
- Optional headless Web mode

## Development

Install Node.js, Rust, the Tauri 2 platform prerequisites, and `cargo-tauri`.

On Linux, screen capture also needs these packages (Debian/Ubuntu names):

```bash
sudo apt install pkg-config libclang-dev libxcb1-dev libxrandr-dev \
  libdbus-1-dev libpipewire-0.3-dev libwayland-dev libegl-dev
```

```bash
npm install
cargo tauri dev -- -- --port 18888 --db-path /tmp/xchat-dev
```

If port `1420` is occupied by an old Vite process, stop that development process before retrying.

React-only preview:

```bash
npm run dev
```

## Build installers

Build bundles for the current platform:

```bash
cargo tauri build
```

Typical macOS outputs:

```text
src-tauri/target/release/bundle/macos/Xchat.app
src-tauri/target/release/bundle/dmg/Xchat_0.1.1_*.dmg
```

Build a specific macOS architecture:

```bash
rustup target add x86_64-apple-darwin
cargo tauri build --target x86_64-apple-darwin

rustup target add aarch64-apple-darwin
cargo tauri build --target aarch64-apple-darwin
```

Other repository targets:

```bash
make help
make deb
make rpm
make apk
make windows-desktop
make web
make web-windows
```

## Optional Web mode

Desktop clients do not need this. Use it only for browser access or a headless host:

```bash
npm run build
cargo run --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features web --bin lanchat-web \
  -- --port 8888 --db-path /tmp/xchat-web
```

The internal Rust package and compatibility binaries remain named `lanchat` / `lanchat-web`. The visible app name, version, and bundle identifier are `Xchat`, `0.1.1`, and `com.xchat.app`.

## Verification

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo check --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features desktop --lib
cargo check --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features web --bin lanchat-web
```

## Data locations

- macOS: `~/Library/Application Support/com.xchat.app/xchat.db`
- Linux: `~/.local/share/com.xchat.app/xchat.db`
- Windows: `%APPDATA%\com.xchat.app\xchat.db`
- Config directory: `xchat`
- Default download directory: `~/Downloads/Xchat`
