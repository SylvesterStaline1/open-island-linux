# Open Island Linux

A floating pill overlay for Linux that shows [Claude Code](https://claude.ai/code) agent status in real time — inspired by [Vibe Island](https://github.com/steipete/open-vibe-island) (macOS only).

Sits at the top of your screen like a Dynamic Island: shows active sessions, what each agent is doing, and lets you **approve or deny tool calls** without switching windows.

![Open Island Linux pill overlay](docs/screenshot.png)

---

## Features

- Floating pill at the top of the screen — always visible, never in the way
- Shows all active Claude Code sessions with their current working directory and phase
- Real-time permission prompts — click **Allow** or **Deny** for tool calls (Bash, Edit, Write, etc.)
- System tray icon with tooltip showing agent count
- Transparent, decoration-free window — blends into your desktop
- Works on KDE Wayland (via XWayland) and X11

---

## Requirements

- Linux with KDE Plasma (Wayland or X11) — other DEs may work but are untested
- [Claude Code](https://claude.ai/code) CLI installed
- Rust toolchain (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Node.js 20+ and [pnpm](https://pnpm.io)
- Tauri 2 system dependencies (WebKit2GTK, etc.)

```bash
# Fedora / RHEL
sudo dnf install webkit2gtk4.1-devel openssl-devel libappindicator-gtk3-devel \
  librsvg2-devel pango-devel cairo-devel gdk-pixbuf2-devel gtk3-devel

# Ubuntu / Debian
sudo apt install libwebkit2gtk-4.1-dev libssl-dev libappindicator3-dev \
  librsvg2-dev libpango1.0-dev libcairo2-dev libgdk-pixbuf2.0-dev libgtk-3-dev
```

---

## Install

```bash
git clone https://github.com/YOUR_USERNAME/open-island-linux
cd open-island-linux
pnpm install
```

---

## Run (development)

```bash
cargo tauri dev
```

This starts the Svelte dev server and the Tauri app together. Rust changes recompile automatically.

---

## Build (release)

```bash
cargo tauri build
```

Binaries land in `target/release/`: `open-island` (the app) and `open-island-hook` (the Claude Code hook relay).

---

## Setup

1. Launch the app — it appears as a pill at the top of the screen and as a tray icon.
2. Right-click the tray icon → **Install Claude Hooks**.  
   This writes hook entries into `~/.claude/settings.json` so Claude Code notifies Open Island of every session and tool call.
3. Start a Claude Code session — it should appear in the pill immediately.

To remove the hooks: right-click tray → **Quit Open Island** (hooks are uninstalled on exit), or re-run and choose uninstall.

---

## How it works

```
Claude Code
  └─ hook fires (PreToolUse / PostToolUse / SessionStart / …)
       └─ open-island-hook binary (reads stdin, writes to Unix socket)
            └─ BridgeServer (inside Tauri process, Unix socket server)
                 └─ forwards events to Svelte frontend via Tauri emit()
                      └─ UI shows session status / permission dialog
                           └─ user clicks Allow / Deny
                                └─ directive sent back through the socket
                                     └─ open-island-hook writes response to stdout
                                          └─ Claude Code sees the decision
```

The hook relay binary blocks until a response arrives (30s timeout) — this is what makes the Allow/Deny flow work synchronously with Claude Code's execution.

---

## Architecture

| Path | Purpose |
|------|---------|
| `src/App.svelte` | Entire UI: pill bar, session list, permission dialog |
| `src-tauri/src/lib.rs` | Tauri setup, window positioning, tray, Tauri commands |
| `src-tauri/src/bridge/server.rs` | Unix socket server, session state machine |
| `src-tauri/src/bridge/protocol.rs` | Wire protocol types (`BridgeEnvelope`, etc.) |
| `src-tauri/src/bridge/state.rs` | `AgentSession`, `SessionPhase` |
| `src-tauri/src/hooks/claude.rs` | Install / uninstall Claude Code hooks |
| `hook-cli/src/main.rs` | Standalone hook relay binary |

---

## Tools that prompt for approval

By default, Open Island intercepts these Claude Code tools and asks for your approval before they run:

`Bash` · `Edit` · `Write` · `MultiEdit` · `NotebookEdit` · `WebFetch` · `WebSearch` · `computer_use`

Edit `requires_approval()` in `src-tauri/src/bridge/server.rs` to change this list.

---

## KDE Wayland note

The app forces `GDK_BACKEND=x11` in `main.rs` to run via XWayland. This is required so GTK honours `set_position()` calls — on native Wayland, window positioning is compositor-controlled and the pill can't be placed at the top of the screen programmatically.

---

## Credits

- Inspired by [open-vibe-island](https://github.com/steipete/open-vibe-island) by [@steipete](https://github.com/steipete)
- Built with [Tauri](https://tauri.app), [Svelte](https://svelte.dev), and [Claude Code](https://claude.ai/code)

---

## License

MIT
