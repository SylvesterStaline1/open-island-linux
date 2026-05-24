---
name: open-island
description: Use this skill when the user says "/open-island", wants to continue working on the Open Island app, or references the floating pill overlay / Claude Code agent status app. Loads full project context and orients Claude for development.
version: 1.3.0
argument-hint: [feature description or issue to work on]
---

# Open Island — Development Context Loader

Open Island is a Tauri 2 + Svelte 5 floating pill overlay that shows Claude Code agent status in real time and lets the user approve/deny tool calls. **Windows is the active commercial target** (Linux port also maintained).

---

## On invocation

1. **Read `CLAUDE.md`** in the project root — authoritative source for architecture, IPC flow, Windows focus implementation, known issues, and current state.

2. **Read the relevant source files** for the task at hand:
   - UI: `src/App.svelte`
   - Tauri backend: `src-tauri/src/lib.rs` (commands, tray, window positioning, focus logic)
   - Bridge server: `src-tauri/src/bridge/server.rs`
   - Bridge protocol/state: `src-tauri/src/bridge/protocol.rs`, `src-tauri/src/bridge/state.rs`
   - Hook install: `src-tauri/src/hooks/claude.rs`
   - Hook relay binary: `hook-cli/src/main.rs` (Windows: terminal HWND enrichment + OSC-0 tab title)
   - Tauri config: `src-tauri/tauri.conf.json`

3. **Check `~/.claude/settings.json`** if the issue relates to hooks or permissions.

4. **Report current state** to the user: what's working, what's pending, and ask what they want to tackle next.

---

## Project file map

| What | Path (from project root) |
|------|--------------------------|
| Architecture docs | `CLAUDE.md` |
| Frontend | `src/App.svelte` |
| Rust lib | `src-tauri/src/lib.rs` |
| Bridge | `src-tauri/src/bridge/` |
| Hooks installer | `src-tauri/src/hooks/claude.rs` |
| Hook relay | `hook-cli/src/main.rs` |
| Tauri config | `src-tauri/tauri.conf.json` |
| Claude settings | `~/.claude/settings.json` |
| TCP port file (Windows) | `%APPDATA%\open-island\port` |
| Hook diagnostic log | `%TEMP%\oi-hook-log.txt` |
| Unix socket (Linux) | `$XDG_RUNTIME_DIR/open-island/bridge.sock` |

---

## Dev workflow

```powershell
# Windows (primary)
cargo tauri dev          # hot-reloads Rust + Svelte
cargo build -p open-island-hook   # rebuild hook relay only
```

```bash
# Linux
cargo tauri dev
```

Hooks auto-install on every app launch (idempotent). If the app seems stuck on an old binary, kill it (Stop-Process / kill), kill any node holding port 5173, then re-run `cargo tauri dev`.

---

## Key Windows-specific facts

- **IPC**: TCP (`127.0.0.1:<port>`), not Unix socket. Port written to `%APPDATA%\open-island\port`.
- **Hook path**: `open-island-hook.exe`; installed with forward slashes in settings.json.
- **Terminal focus**: Hook captures terminal HWND via process-tree walk (skips shells, finds `WindowsTerminal.exe` etc.). `focus_session_windows` in lib.rs uses **AttachThreadInput** to bypass overlay foreground restriction, then `SetForegroundWindow` + UIA tab select.
- **UIA tab matching**: Hook writes `OI-{session_id[..8]}` as WT tab title at SessionStart via OSC-0 escape to CONOUT$. Click matches tab by that token.
- **OOTB constraint**: No external tools, no elevated rights, no manual steps — must work for any end user.
- **windows crate 0.58**: `AttachThreadInput` → `Win32::System::Threading`; `SetFocus` → `Win32::UI::Input::KeyboardAndMouse`; NOT in `Win32::UI::WindowsAndMessaging`.
