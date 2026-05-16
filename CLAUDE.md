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

After building, install Claude hooks via the system tray → "Install Claude Hooks".

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

## Pill UI states

| State | Window size | Trigger |
|-------|------------|---------|
| **Idle/collapsed** | 200×38px | No active sessions, no permission |
| **Expanded** | 600×60px | User click, OR permission request |
| **Expanded + sessions** | 600×(60 + n×58)px | Active sessions visible |
| **Expanded + permission** | 600×170px | Approval required |

- Collapsed pill: dot + "Open Island" (11px) — nearly invisible when idle
- Expanded pill: dot + brand + session chips (cwd paths) + chevron
- Permission panel: tool name + args + Deny/Allow buttons
- Session list: shown below pill when expanded and sessions active
- Click toggles expanded (except when permission is pending — auto-expands)
- Spring animation on height: `cubic-bezier(0.34, 1.56, 0.64, 1)` 0.32s
- Content cross-fades between collapsed/expanded layers via opacity

## CSS constants (App.svelte)
- `WIN_W = 600` — expanded window width (logical px)
- `WIN_W_IDLE = 200` — collapsed window width (logical px)
- `PILL_H = 60` — expanded pill height
- `PILL_H_IDLE = 38` — collapsed pill height
- `SESSION_H = 58` — per-session row height in list
- `PERMISSION_H = 108` — permission panel height
- Font sizes: 11px collapsed brand, 12px expanded brand (intentionally below 13px)

## Window / display
- Window starts hidden, `"center": true` in tauri.conf.json for WM initial placement
- **Key Tauri command**: `set_window_geometry(width, height)` — resizes AND recenters in one call. Called from `$effect` whenever `isExpanded` or session count changes. Does an immediate set_position + 80ms delayed retry.
- **`primary_top_center(win, width)`** in lib.rs: computes (x, y) centered on the primary monitor, y = monitor_top + KDE panel height
- **Panel height**: read from `~/.config/plasmashellrc` → `thickness=28`. `_NET_WORKAREA` does NOT work on this KDE Wayland + XWayland setup.
- **KDE Wayland fix**: `GDK_BACKEND=x11` in `main.rs` forces XWayland so GTK honours `set_position`. Never remove this.
- Always use `LogicalSize` / `LogicalPosition` — never PhysicalSize for window dimensions.
- JS positioning removed from `onMount` — Rust is the single source of truth for window placement.
- Startup sequence: `position_at_top` before `win.show()`, then 300ms delayed retry.

## Multi-monitor setup (this machine)
- External DP-2: 2496×1404 at global y=0 (top screen, NOT primary)
- Laptop eDP-1: 1920×1200 at global y=1404 (PRIMARY — where pill lives)
- `primary_monitor()` correctly returns eDP-1. Do NOT use "topmost by y" heuristic — that's the external monitor which the user does NOT want.

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

## Current state (as of 2026-05-16)
- Sessions appear correctly in the pill
- Permission Allow/Deny flow works end-to-end
- `dangerouslySkipPermissions: true` in `~/.claude/settings.json` prevents double-prompting
- Pill collapses to small 200×38px capsule when idle, expands on click or permission
- **Pending**: Confirm pill appears correctly positioned on eDP-1 primary monitor just below the 28px KDE panel after latest positioning fixes

## Known issues / gotchas
- `_NET_WORKAREA` returns y=0 on KDE Wayland + XWayland — useless for panel detection. Use `~/.config/plasmashellrc` `thickness=` instead.
- `current_monitor()` returns None when called before the WM maps the window — use `primary_monitor()` instead.
- `set_size(PhysicalSize(w, h))` on a 2x display creates a half-logical-pixel window — always use `LogicalSize`.
- Terminal may be left in raw mode after a Tauri panic — run `reset` to fix.
- The user has two monitors: external DP-2 (top, NOT primary) and laptop eDP-1 (bottom, PRIMARY). The pill goes on eDP-1. Do NOT switch to a "topmost monitor" heuristic.
- `set_window_geometry` spawns a delayed 80ms repositioning task — do not call it in a tight loop.

## Svelte 5 notes
- Uses rune API: `$state`, `$derived`, `$effect`
- `mount()` from `svelte` (not `new App()`) — vite.config.ts has `resolve: { conditions: ["browser", ...] }`
- `$effect` runs before `onMount` in the first render cycle
- Effects CAN set `$state` variables (e.g. auto-expanding on permission) without loops if the state change doesn't re-trigger the effect's dependencies
