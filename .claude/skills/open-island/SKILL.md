---
name: open-island
description: Use this skill when the user says "/open-island", wants to continue working on the Open Island Linux app, or references the floating pill overlay / Claude Code agent status app. Loads full project context and orients Claude for development.
version: 1.2.0
argument-hint: [feature description or issue to work on]
---

# Open Island — Development Context Loader

Open Island is a Tauri 2 + Svelte 5 app: a floating pill overlay at the top of the screen (like the Mac Dynamic Island) that shows Claude Code agent status in real time and lets the user approve/deny tool calls. Targets Linux now, Windows commercially.

---

## On invocation

1. **Read `CLAUDE.md`** in the project root — it is the authoritative source of architecture, file map, IPC flow, known issues, current state, and Windows port plan.

2. **Read the relevant source files** for the task at hand (paths relative to project root):
   - UI: `src/App.svelte`
   - Tauri backend: `src-tauri/src/lib.rs`
   - Bridge server: `src-tauri/src/bridge/server.rs`
   - Hook install: `src-tauri/src/hooks/claude.rs`
   - Hook relay binary: `hook-cli/src/main.rs`

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
| Socket (runtime) | `$XDG_RUNTIME_DIR/open-island/bridge.sock` |

---

## Dev workflow

```bash
cargo tauri dev          # watches both Rust and Svelte, hot-reloads frontend
```

Rust changes auto-recompile in dev mode. If the app seems stuck on an old binary, kill and restart `cargo tauri dev`.

Hook relay only:
```bash
cargo build -p open-island-hook
```

After building, hooks are auto-installed on every app launch (idempotent). Manual install: system tray → "Install Claude Hooks".
