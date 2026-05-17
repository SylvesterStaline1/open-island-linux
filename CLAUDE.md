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

## Design constraints — OOTB (out-of-the-box)

Open Island is intended for distribution to anyone. **Every feature must work on a clean Linux install with zero manual configuration by the user.** Specifically:
- Do NOT require `ydotool`, `wmctrl`, `xdotool`, or any keyboard-injection tool.
- Do NOT require `usermod -aG input`, `/dev/uinput` access, or `TIOCSTI`/sysctl changes.
- Do NOT require shell rc edits, environment variable exports, or system daemons.
- Hooks are auto-installed on every launch (idempotent). Hooks survive app restarts.
- "Uninstall Hooks & Quit" is the only way to remove hooks — plain "Quit" leaves them intact.

When proposing a new feature: if it needs any of the above, find an OOTB alternative first.

## IPC flow
1. Claude Code fires a hook (PreToolUse, PostToolUse, SessionStart, etc.)
2. `open-island-hook <EventName>` binary is called with JSON payload on stdin
3. The hook relay connects to the Unix socket and sends `BridgeEnvelope::Command { ProcessClaudeHook }`
4. `BridgeServer` (in Tauri process) handles it, updates session state, emits `ServerEvent`
5. `forward_events()` in lib.rs relays ServerEvents to the frontend via `app.emit()`
6. For PreToolUse on gated tools: hook relay **blocks** waiting for a user decision from either:
   - (a) A keypress on `/dev/tty` — the hook prints its own approval prompt and reads `y`/`n`
   - (b) A `BridgeResponse::ClaudeHookDirective` pushed over the still-open socket when the user clicks Allow/Deny in the pill
7. Whichever fires first wins. Hook writes `{"permissionDecision":"allow"|"deny"}` (or `"ask"` on 30s timeout) to stdout and exits.
8. `resolve_permission` Tauri command → `ServerInner::resolve_permission` → sends on `pending_hook_decisions` oneshot → triggers path (b) above.

**No keyboard injection, no wmctrl, no ydotool.** The hook owns both the tty prompt and the socket wait.

## Unix socket
- Path: `$OPEN_ISLAND_SOCKET_PATH` → `$VIBE_ISLAND_SOCKET_PATH` → `$XDG_RUNTIME_DIR/open-island/bridge.sock`
- Default on this machine: `/run/user/1000/open-island/bridge.sock`
- Protocol: newline-delimited JSON (`BridgeEnvelope` tagged union)

## Tools that block for approval (`requires_approval`)
`Bash`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`, `WebFetch`, `WebSearch`, `computer_use`

## Pill UI states

| State | Window size | Trigger |
|-------|------------|---------|
| **Sliver** | 480×44px (only 14px visible) | Not hovered, no urgent session — pill pushed up via CSS transform |
| **Hover** | 480×44px | Mouse enters window (250ms debounce to collapse) |
| **Expanded + sessions** | 480×(44 + 8 + panel_height)px | Click or urgent session — panel slides in below pill |

- **Sliver** (idle, not hovered, not urgent): `.root` has `transform: translateY(-30px)`. Only the bottom 14px of the pill peeks below the KDE panel.
- **Sliver always** when `!isHovered && !userExpanded && !isAwaiting` — even if working sessions exist (users hover to check).
- **Hover** → `isHovered = true` → pill slides fully into view. 250ms debounce on mouse leave.
- **Panel** (below pill): 480px wide, `#0A0A0A`, radius 18. Three variants: **session list** (default), **code approval** (tool awaiting permission), **question** (AskUserQuestion, dormant v1).
- **Urgent** → auto-expands and holds open panel to the diff/question.
- Pill is `inline-flex` content-sized (not full 480), centered in the window. Overlapping ToolBadges (size 26, `marginLeft:-8`). Urgent badge is index 0: red bg + `oi-ring` pulse. Chevron on the RIGHT, rotates 180° when expanded.
- Easing: `cubic-bezier(0.2, 0, 0, 1)` everywhere (no spring).

## CSS constants (App.svelte)
- `WIN_W = 480` — window width (always, even at sliver)
- `PILL_H = 44` — pill height
- `SLIVER_OFFSET = 30` — translateY(-30) at rest → 14px of pill visible
- `PANEL_GAP = 8` — gap between pill bottom and panel top
- Pill background `#0A0A0A`, border-radius `0 0 12px 12px`, padding `0 16px`
- Panel: `bg #0A0A0A, radius 18, shadow 0 12px 32px rgba(0,0,0,0.65)`

## Sliver hover mechanic (important)
- `isSliver = $derived(!isHovered && !userExpanded && !isAwaiting)` — no active-count condition; pill always rests at sliver unless urgent or explicitly expanded.
- `urgentSession = $derived(activeSessions.find(s => s.pending_permission) ?? activeSessions.find(s => s.pending_question) ?? null)`
- `isAwaiting = $derived(urgentSession !== null)`
- `panelVariant = $derived(urgentSession?.pending_permission ? "code" : urgentSession?.pending_question ? "question" : "list")`
- Hover handlers are on `.hover-wrapper` (outer, no transform) NOT on `.root` (transformed). CSS transforms shift pointer-event hit areas.
- `.hover-wrapper` has no transform → its hit area is always the full window height → reliable mouseenter.

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

## Current state (as of 2026-05-17)
- Nothing DS redesign implemented: overlapping badges, oi-ring pulse, code-approval panel with diff snippet, question panel (dormant until backend wires AskUserQuestion).
- Sessions appear correctly in the pill. Permission flow works end-to-end (hook-owns-both-UIs architecture).
- Hooks auto-install on every app launch; they survive plain "Quit" (only "Uninstall Hooks & Quit" removes them).
- `dangerouslySkipPermissions` is NOT set and NOT needed.
- Sliver mode: pill rests at sliver (`translateY(-30)`, 14px visible) whenever not hovered/urgent. Hover → full expansion. Mouse leave (250ms debounce) → back to sliver.
- Window is always 480px wide. Panel reveals below pill with `max-height/opacity/transform` transitions; window height grows immediately on open, shrinks after 470ms (after close animation).
- Urgent permission → panel auto-opens to code-approval variant with `buildDiff` showing tool-specific content. **OPEN TERMINAL** button focuses the right terminal window. The two-button Deny/Allow row is commented out, labeled "Windows port: WriteConsoleInput".
- **Pending**: Confirm pill positioned correctly on eDP-1 primary monitor just below the 28px KDE panel.

## Dot-glyph system (App.svelte)
- `TOOL_GLYPHS: Record<string, string>` — 12×12 grid strings (144 chars, '.' = empty, '#' = filled), stored as concatenated row strings (no whitespace stripping needed).
- `dotGlyph(key, size, color)` — returns inline SVG string; use `{@html dotGlyph(...)}` in templates. Grid: 12×12 cells, cell=10px, r=3.25px (65% of cell).
- `TOOL_GLYPH_MAP` — maps `Bash`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`, `WebFetch`, `WebSearch`, `Read` to glyph keys.
- `primaryGlyph` — derived: permission tool glyph > `'bash'` (active) > `'power'` (idle).
- `sessionGlyph(s)` — per-session: pending_permission tool > `'check'` > `'bash'` > `'power'`.
- Available glyph keys: `bash`, `edit`, `write`, `multiedit`, `notebook`, `webfetch`, `websearch`, `read`, `check`, `alert`, `power`, `question`, `chevronDown`, `chevronUp`, `close`, `play`.
- Pulsing: wrap glyph `{@html ...}` in `<span class:pulsing={condition}>` — the `.pulsing` CSS class applies opacity animation.

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
