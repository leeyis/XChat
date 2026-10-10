# Xchat

Xchat `0.1.14` is a LAN chat client built with Tauri 2, React, and Rust. Install and run the client on each device; discovery, messaging, file transfer, and SQLite storage are built in, so normal desktop use does not require a separate server.

## Features

- Automatic LAN discovery and manually configured hosts
- Direct and group chats, offline delivery, delivered/read receipts
- Four-way parallel transfer for large files, with resume, cancellation, retry, and a file center
- Pasted, selected, and dropped image drafts with inline message rendering
- Inline audio/video playback and GIF, animated WebP, and APNG previews, with pause controls and original-file actions
- Shared Web/desktop capture editor with adjustable selections, multiline text editing and dragging, shape and brush tools, mosaic, blur, eraser, undo/redo, and mouse-wheel sizing
- Direct region capture and editable pins that retain their screen position, with zoom, rotation, opacity, window-shadow controls, and hide/recover actions; Web pins stay inside the page
- Chinese/English UI, themes, notifications, download, network, and local IP/MAC identity settings
- Optional headless Web mode
- Remote assistance in a separate fullscreen-capable viewer, a compact sharing toolbar, session-scoped control permission, and voice calls independent of chat layout
- Windows DXGI capture and hardware HEVC delivery when supported, with compatible RTP fallback
- Desktop launch-at-login settings and online-first group member selection

The capture button and configured shortcut open region selection directly. In the capture editor, Enter inserts a new text line and clicking outside commits the edit. Drag a text box by its border; Delete removes the active annotation. The mouse wheel adjusts the current tool size. Right-click a pin for its menu; Space starts annotation while preserving its position and scale. Ctrl+Shift+P enables click-through, F3 restores hidden or click-through pins, Esc hides a pin, and Shift+Esc destroys it. Disabling the window shadow also removes the pin border.

## Development

Remote performance remains under validation. The monitored Windows RDP test delivered about 31.4 FPS at 1080p, including while minimized, with 164.68 ms mean source-pixel age. This same-machine result does not certify two-device LAN performance or macOS microphone/hardware decoding; see the [verification record](docs/verification/2026-10-10-remote-followup.md).

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

On Windows, `tauri.windows.conf.json` builds the frontend into `dist/frontend` and embeds it through a relative path. Keep `frontendDist` relative: a Windows drive path can be parsed as a URL and leave the installer without bundled frontend assets.

Check the embedded entry point and its JavaScript, CSS, and icon before packaging:

```bash
npm run build -- --outDir ../dist/frontend --emptyOutDir
cargo test --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features desktop,custom-protocol --test bundled_frontend
```

Typical macOS outputs:

```text
src-tauri/target/release/bundle/macos/Xchat.app
src-tauri/target/release/bundle/dmg/Xchat_0.1.14_*.dmg
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

The internal Rust package and compatibility binaries remain named `lanchat` / `lanchat-web`. The visible app name, version, and bundle identifier are `Xchat`, `0.1.14`, and `com.xchat.app`.

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
