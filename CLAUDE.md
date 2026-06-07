# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rustyfoot is a Rust reimplementation of the MOD Audio `mod-ui` web server. It provides a web-based interface for managing LV2 audio plugins, pedalboards, and effects on Linux audio devices (primarily Raspberry Pi / ARM targets, also x86_64). It communicates with `mod-host` (the audio engine) over TCP sockets and serves a browser-based UI.

## Build Commands

```bash
make              # Build C utility library + Rust binary (debug)
make release      # Build C utility library + Rust binary (release)
make run          # Build and run locally
make cross-pi4    # Cross-compile for aarch64 (Raspberry Pi 4)
make clean        # Clean everything

# Rust-only (after lib is built)
LD_LIBRARY_PATH=./lib cargo build
LD_LIBRARY_PATH=./lib cargo run
LD_LIBRARY_PATH=./lib cargo check   # Fast type-checking
```

The C library (`libmod_utils.so`) in `utils/` must be built first — it wraps lilv (LV2) and JACK APIs. The Rust binary loads it at runtime via `libloading` (FFI).

## Architecture

### Core Layers

1. **Web Layer** (`src/web/`) — Actix-web HTTP handlers organized by domain in `src/web/handlers/`. All routes are registered in `src/main.rs`. Static files served from `html/`.

2. **Session** (`src/session.rs`) — Central state broker. Holds the `Host`, `Hmi`, `Recorder`, `Player`, `ScreenshotGenerator`, and WebSocket connections. Wrapped in `Arc<RwLock<Session>>` (`SharedSession`). Most web handlers acquire a write lock on the session to perform operations.

3. **Host** (`src/host/`) — Interface to the `mod-host` audio engine, split into submodules:
   - `ipc` — TCP socket communication (command port N, notification port N+1), message queue with async send/receive
   - `mapper` — Bidirectional mapping between plugin instance names (e.g. `/graph/plugin_1`) and numeric IDs
   - `plugin` — Plugin data structures and port state
   - `connections` — Audio/MIDI port connection tracking
   - `pedalboard` — Pedalboard state, snapshots, TTL serialization
   - `transport` — BPM, beats-per-bar, sync, rolling state

4. **LV2 Utils** (`src/lv2_utils.rs`) — FFI wrapper around `libmod_utils.so`. Loads the C library via `libloading` and exposes functions for plugin introspection, pedalboard parsing (from TTL files), and JACK port management. Heavy use of `unsafe` for C struct marshalling.

5. **HMI** (`src/hmi.rs`) — Hardware UI communication (TCP or serial to physical device). `FakeHmi` in `src/development.rs` for dev mode.

6. **Store** (`src/store/`) — Plugin store with Patchstorage and Tone3000 backends for downloading plugins/models.

7. **Communication** (`src/communication/`) — Device authentication, RSA/AES crypto, token management.

### Configuration

All settings come from environment variables with `MOD_` prefix (see `src/settings.rs`). Key env vars:
- `MOD_DEV_ENVIRONMENT=1` — enables dev mode (fake HMI, fake host)
- `MOD_DEV_HOST=1` / `MOD_DEV_HMI=1` — individual dev stubs
- `MOD_DESKTOP=1` — binds to 127.0.0.1 instead of 0.0.0.0
- `MOD_DEVICE_WEBSERVER_PORT` — HTTP port (default 8888)
- `MOD_DEVICE_HOST_PORT` — mod-host TCP port (default 5555)
- `MOD_DATA_DIR` — persistent data directory
- `RUST_LOG` — tracing log level filter

### Key Patterns

- Ported from Python (`mod-ui`), so many module names and API paths mirror the original codebase
- Uses Rust 2024 edition
- WebSocket broadcast channel (`tokio::sync::broadcast`) pushes real-time state updates to all connected browser clients
- Plugin data flows as `serde_json::Value` throughout (matching the dynamic JSON nature of LV2 metadata)
- Pedalboards are LV2 bundles (directories ending in `.pedalboard/` containing TTL files)
- The `AppState` struct in `main.rs` holds all shared state passed to handlers via `actix_web::web::Data`

### Rustyfoot Repositories

All repositories live side-by-side under the same parent directory (`../`):

| Repository | Path | Purpose |
|------------|------|---------|
| **rustyfoot** | `./` | Main web server — Rust binary + C FFI library. Serves the browser UI, talks to mod-host over TCP, manages pedalboards/plugins/banks/profiles. |
| **rustyfoot-hmi** | `../rustyfoot-hmi` | Touch-screen UI — Flutter app via flutterpi, runs directly on the Pi. Communicates with rustyfoot over TCP (port 9898). |
| **rustyfoot-modhost** | `../rustyfoot-modhost` | Audio engine packaging — Debian package wrapping mod-host, mod-midi-merger, and jackd with systemd units and JACK session config. |
| **rustyfoot-builder** | `../rustyfoot-builder` | Plugin build system — builds LV2 plugins from source using YAML descriptors, generates modgui (web pedal UI), packages as tarballs. Also builds VST3 plugins for the VST3 bridge. |
| **rustyfoot-vst3** | `../rustyfoot-vst3` | VST3 bridge add-on — provides `vst3-wrapper.so` (shared LV2 wrapper using Carla backend) and `rustyfoot-vst3-scan` (VST3 introspection tool). Enables VST3 plugins to appear as native LV2 pedals. |

### Deployment

Packaged as a Debian package (`debian/`). Runs as a systemd service (`debian/rustyfoot.service`) under the `modhost` user. Requires `mod-host` and JACK to be running.

### Target Device (Raspberry Pi 5 — "tatooine")

**SSH access:** `ssh pi@tatooine` (or `pi@192.168.1.37`)

**Key paths on the Pi:**
- Data directory: `/var/lib/rustyfoot/`
- Pedalboards: `/var/lib/rustyfoot/pedalboards/`
- Plugins (LV2 bundles): `/var/lib/rustyfoot/plugins/` (owned by `modhost:modhost`)
- Rustyfoot binary: `/usr/bin/rustyfoot`
- Rustyfoot C library: `/usr/lib/rustyfoot/libmod_utils.so`
- Source repos: `~/src/rustyfoot`, `~/src/rustyfoot-hmi`, `~/src/rustyfoot-modhost`

**Services:**
```bash
sudo systemctl restart rustyfoot           # Restart rustyfoot
sudo systemctl restart jackd-modhost       # Restart JACK + mod-host
sudo systemctl restart jackd-modhost && sudo systemctl restart rustyfoot  # Full restart
journalctl -u rustyfoot -f                 # Follow rustyfoot logs
journalctl -u jackd-modhost -f             # Follow JACK/mod-host logs
```

**Building and deploying on the Pi:**
```bash
cd ~/src/rustyfoot && git pull && dpkg-buildpackage -b -us -uc
sudo dpkg -i ~/src/rustyfoot_0.1.0-1_arm64.deb
```
Always use `dpkg-buildpackage` + `dpkg -i` to build and install — this ensures the binary, C library, HTML/JS assets, default pedalboards, and service file are all installed to their correct locations. Do not manually copy individual files.

**Network:** Ethernet + WiFi. IPv6 disabled system-wide. Ethernet may be unplugged when mobile (WiFi provides connectivity). Post-install setup documented in `POST_INSTALL.md`.
