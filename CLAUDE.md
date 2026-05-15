# Open Island Linux — Project Context

## What this is
Linux port of Vibe Island (macOS-only). A floating pill overlay at the top of the screen (like the Mac Dynamic Island) that shows Claude Code agent status and lets the user approve/deny tool calls in real time.

Built with **Tauri 2** (Rust backend + WebKit2GTK) and **Svelte 5** (frontend).

## How to run
```bash
cd /home/sstaline/open-island-linux
cargo tauri dev          # builds Rust + starts Svelte dev server
# or build release:
cargo tauri build
```

The hooks binary is a separate crate:
```bash
cargo build -p open-island-hook   # rebuilds just the hook relay
```

After building, install Claude hooks via the system tray → "Install Claude Hooks", or:
```bash
# tray menu is the preferred way; hooks write to ~/.claude/settings.json
```

## Architecture

```
open-island-linux/
├── src/                    # Svelte 5 frontend
│   └── App.svelte          # entire UI: pill bar, session list, permission dialog
├── src-tauri/src/
│   ├── main.rs             # forces GDK_BACKEND=x11 before GTK init (XWayland fix)
│   ├── lib.rs              # Tauri setup, commands, tray, window positioning
│   ├── bridge/
│   │   ├── protocol.rs     # BridgeEnvelope / BridgeCommand / ClaudeHookPayload types
│   │   ├── server.rs       # Unix socket server, session state machine, permission flow
│   │   └── state.rs        # AgentSession, SessionPhase, BridgeState
│   └── hooks/
│       └── claude.rs       # install/uninstall hooks into ~/.claude/settings.json
└── hook-cli/src/main.rs    # Standalone binary: Claude → socket → bridge relay
```

## IPC flow
1. Claude Code fires a hook (PreToolUse, PostToolUse, SessionStart, etc.)
2. `open-island-hook <EventName>` binary is called with JSON payload on stdin
3. The hook relay connects to the Unix socket and sends `BridgeEnvelope::Command { ProcessClaudeHook }`
4. `BridgeServer` (in Tauri process) handles it, updates session state, emits `ServerEvent`
5. `forward_events()` in lib.rs relays ServerEvents to the frontend via `app.emit()`
6. For PreToolUse on blocking tools: hook relay BLOCKS waiting for a oneshot channel response (30s timeout)
7. User clicks Allow/Deny in the pill → `resolve_permission` Tauri command → directive sent back to hook relay → hook relay writes `{"decision":"block","reason":"..."}` or nothing to stdout

## Unix socket
- Path: `$OPEN_ISLAND_SOCKET_PATH` → `$VIBE_ISLAND_SOCKET_PATH` → `$XDG_RUNTIME_DIR/open-island/bridge.sock`
- Default on this machine: `/run/user/1000/open-island/bridge.sock`
- Protocol: newline-delimited JSON (`BridgeEnvelope` tagged union)

## Tools that block for approval (`requires_approval`)
`Bash`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`, `WebFetch`, `WebSearch`, `computer_use`

## Window / display
- Window: 460px logical width, `decorations: false`, `transparent: true`, `alwaysOnTop: true`, `skipTaskbar: true`
- **KDE Wayland fix**: `GDK_BACKEND=x11` in `main.rs` forces XWayland so GTK honours `set_position`
- Positioning: `position_at_top()` in lib.rs uses `primary_monitor()` (not `current_monitor()` — that returns None before window is mapped). A 150ms delayed re-position fires after startup as belt-and-suspenders.
- Dynamic height: `set_window_height` Tauri command called from `$effect` in App.svelte. **Must use `LogicalSize`** — using `PhysicalSize(460, h)` halves the visible width on HiDPI displays (2x scale).
- Frontend also re-positions in `onMount` via JS `getCurrentWindow().currentMonitor()` with `availableMonitors()` fallback.

## Hook format in ~/.claude/settings.json
Claude Code expects:
```json
{
  "PreToolUse": [
    {
      "matcher": "",
      "hooks": [{ "type": "command", "command": "OPEN_ISLAND_SOCKET_PATH=... open-island-hook PreToolUse" }]
    }
  ]
}
```
The `matcher` wrapper is required — bare `{ "type": "command" }` objects are silently ignored.
Hook event names from Claude Code are PascalCase (`SessionStart`, not `sessionStart`). The server lowercases before matching.

## Current state (as of 2026-05-15)
- Sessions appear correctly in the pill
- Permission Allow/Deny flow works end-to-end
- `dangerouslySkipPermissions: true` in `~/.claude/settings.json` prevents double-prompting from Claude's own dialog
- **Pending**: Window centering + full-width rendering on HiDPI displays (the LogicalSize + primary_monitor fixes are in but need a fresh `cargo tauri dev` build to confirm)

## Known issues / gotchas
- `current_monitor()` returns None when called before the WM maps the window — use `primary_monitor()` instead
- `set_size(PhysicalSize(460, h))` on a 2x display creates a 230-logical-pixel window, clipping the right half of the pill — always use `LogicalSize`
- Terminal may be left in raw mode after a Tauri panic — run `reset` to fix
- VS Code Up-arrow history: `"terminal.integrated.suggest.enabled": false` in settings.json

## Svelte 5 notes
- Uses rune API: `$state`, `$derived`, `$effect`
- `mount()` from `svelte` (not `new App()`) — vite.config.ts has `resolve: { conditions: ["browser", ...] }`
- `$effect` runs before `onMount` in the first render cycle
