# Open Island — Project Context

## What this is
A floating pill overlay at the top of the screen (like the Mac Dynamic Island) that shows Claude Code agent status and lets the user approve/deny tool calls in real time.

Built with **Tauri 2** (Rust backend) and **Svelte 5** (frontend). Originally Linux-only; now being actively ported to **Windows** as the primary commercial platform.

## How to run

### Windows (active development)
```powershell
cd C:\Users\wilan\documents\open-island-linux
cargo tauri dev          # builds Rust + starts Svelte dev server, hot-reloads
cargo tauri build        # release build
```

### Linux
```bash
cd /home/sstaline/open-island-linux
cargo tauri dev
```

Hook relay binary (separate crate — rebuild after changes):
```bash
cargo build -p open-island-hook
```

Hooks are **auto-installed on every app launch** (idempotent). Manual reinstall: system tray → "Install Claude Hooks".

## Architecture

```
open-island-linux/
├── src/                        # Svelte 5 frontend
│   └── App.svelte              # entire UI: pill bar, session list, permission/approval panels
├── src-tauri/src/
│   ├── main.rs                 # Linux: forces GDK_BACKEND=x11 (XWayland fix)
│   ├── lib.rs                  # Tauri setup, commands, tray, window positioning,
│   │                           #   focus_session_terminal (Windows: AttachThreadInput + UIA)
│   ├── bridge/
│   │   ├── protocol.rs         # BridgeEnvelope / BridgeCommand / ClaudeHookPayload types
│   │   ├── server.rs           # TCP bridge server, session state machine, permission flow
│   │   └── state.rs            # AgentSession, SessionPhase, BridgeState
│   └── hooks/
│       └── claude.rs           # install/uninstall hooks into ~/.claude/settings.json
└── hook-cli/src/main.rs        # Standalone binary: CC hook → TCP → bridge relay
                                #   Windows: enriches payload with terminal HWND,
                                #            sets WT tab title via OSC-0 at SessionStart
```

## Design constraints — OOTB (out-of-the-box)

Open Island is intended for distribution to anyone. **Every feature must work with zero manual configuration by the user.** Specifically:
- Do NOT require external tools (ydotool, wmctrl, xdotool, xdg-open, etc.).
- Do NOT require shell rc edits, env var exports, or system daemons.
- Hooks auto-install on every launch. Hooks survive plain "Quit" (only "Uninstall Hooks & Quit" removes them).
- On Windows: no elevated rights, no registry edits by the user, no WSL dependency.

## IPC flow

### Windows
1. Claude Code fires a hook (PreToolUse, PostToolUse, SessionStart, etc.)
2. `open-island-hook.exe <EventName>` is called with JSON payload on stdin
3. Hook reads the TCP port from `%APPDATA%\open-island\port`
4. Hook connects to `127.0.0.1:<port>` and sends `BridgeEnvelope::Command { ProcessClaudeHook }` (newline-delimited JSON)
5. **Windows-only enrichment** (before sending): hook walks the process tree to find the terminal host (WindowsTerminal.exe etc.), captures its HWND, injects `terminal_window_id`, `terminal_app`, `terminal_pid`, `terminal_session_id` into the payload. At SessionStart under WT, also writes `OI-{8chars}` tab title via OSC-0 to CONOUT$.
6. `BridgeServer` handles it, updates session state, emits `ServerEvent`
7. `forward_events()` in lib.rs relays ServerEvents to the frontend via `app.emit()`

### Linux
Same flow but uses a **Unix socket** instead of TCP:
- Path: `$OPEN_ISLAND_SOCKET_PATH` → `$VIBE_ISLAND_SOCKET_PATH` → `$XDG_RUNTIME_DIR/open-island/bridge.sock`
- Default: `/run/user/1000/open-island/bridge.sock`

### Permission flow (gated tools)
For PreToolUse on gated tools (Bash, Edit, Write, etc.):
- Hook returns `"ask"` to stdout → CC shows its native `1. Yes / 2. No` prompt in the terminal
- **Linux**: hook polls `/dev/tty` for keypresses while connected to bridge (dual-channel wait via `poll()` on tty fd + socket fd)
- **Windows**: hook spawns a keypress reader thread on `CONIN$` (raw console mode) while the main thread reads socket lines for directives. Whichever channel returns first wins — `allow`/`deny` decision returned to CC. Pill buttons emit `resolve_permission` → bridge sends `ClaudeHookDirective` back to the hook over the same TCP connection.

## TCP bridge (Windows)

- Port: written by bridge server to `%APPDATA%\open-island\port` on startup
- Hook reads this file to find the port (no env var needed — zero config)
- Protocol: newline-delimited JSON (`BridgeEnvelope` tagged union)
- The bridge accepts multiple simultaneous connections (one per hook invocation)

## Hook format in `~/.claude/settings.json`

```json
{
  "PreToolUse": [
    {
      "matcher": "",
      "hooks": [{ "type": "command", "command": "C:/path/to/open-island-hook.exe PreToolUse" }]
    }
  ]
}
```

- The `matcher` wrapper is required — bare `{ "type": "command" }` objects are silently ignored by CC.
- Hook event names from CC are PascalCase (`SessionStart`, not `sessionStart`). The server lowercases before matching.
- Hook binary path uses **forward slashes** (works in cmd.exe, PowerShell, and sh/bash on Windows).
- CC reads `settings.json` **once at process startup** — hooks only apply to CC sessions started after the app installs them.
- Diagnostic log: `%TEMP%\oi-hook-log.txt` — append-only, one line per invocation, shows `ts=... event=... hwnd=... app=...`. Remove before release.

## Files changed (2026-05-24 session)

1. **`src/App.svelte`** — Added `handlePermission()` function, wired Allow/Deny buttons in permission panel, added `.btn-allow`, `.btn-deny`, `.action-row-secondary` styles. **Window sizing fix**: added `pillEl` ref + `pillWidth` state + `ResizeObserver` on `.pill` element; updated window geometry effect to compute `targetW` based on measured pill width in sliver mode (shrinks from 480px to ~pill-width + 8px); `appliedHeight` initialized to `PILL_H - SLIVER_OFFSET` (14px) instead of `PILL_H` (44px). **Clean sliver mode**: badges, count, and chevron are now hidden in sliver mode — only clean black pill shows.
2. **`hook-cli/src/main.rs`** — Fixed `ConsoleModeGuard` to use `CONSOLE_MODE` type (matching `windows` crate 0.58`), removed duplicate `impl` blocks, fixed `HANDLE` `Send` issue by passing raw `isize` handle value to thread closure, added `use std::io::Write` for `set_tab_title`, fixed match patterns to use literal `u16` values instead of `b'x' as u16`.
3. **`src-tauri/tauri.conf.json`** — Temporarily changed `beforeDevCommand` and `beforeBuildCommand` to `"echo skip"` to bypass pnpm/node_modules issues during development. Window config restored to original (removed `"shadow": false` that was added and then removed).

## Tools that block for approval (`requires_approval`)
`Bash`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`, `WebFetch`, `WebSearch`, `computer_use`

## Windows terminal focus (`focus_session_terminal`)

When the user clicks a session row, the pill calls `focus_session_terminal(sessionId)` → Tauri command → spawns thread → `focus_session_windows(session)`.

### How HWND is captured (hook-cli/src/main.rs — `win_terminal` mod)
- Uses `CreateToolhelp32Snapshot` to build a PID→(PPID, name) map
- Walks parent chain: hook → claude.exe → shell (skipped) → terminal host
- **TERMINAL_APPS** (graphical hosts only — shells excluded): `WindowsTerminal.exe`, `OpenConsole.exe`, `conhost.exe`, `alacritty.exe`, `wezterm-gui.exe`, `mintty.exe`, `Hyper.exe`, `Tabby.exe`
- Shells (`powershell.exe`, `cmd.exe`, `pwsh.exe`, etc.) are NOT in the list — they have no window
- If parent-chain walk fails, fallback: look for `conhost.exe` child of the shell PID
- `hwnd_for_pid()`: `EnumWindows` → first visible top-level window owned by that PID
- Session stores: `terminal_window_id` (HWND as string), `terminal_app`, `terminal_pid`, `terminal_session_id` (WT_SESSION GUID)

### How focus is applied (lib.rs — `focus_session_windows`)
- Parses `terminal_window_id` as isize → HWND
- **AttachThreadInput trick**: attaches our thread's input queue to both the current foreground thread AND the target window's thread — bypasses Windows' foreground-window restriction (overlay windows with `alwaysOnTop: true` are not granted foreground rights by default; `SetForegroundWindow` silently fails without this).
- Calls `ShowWindow(SW_RESTORE)` + `BringWindowToTop` + `SetForegroundWindow` + `SetFocus`
- Detaches thread input
- For `WindowsTerminal.exe`: runs UIA tab selection (`select_wt_tab_by_title`) — finds the tab whose name contains `OI-{8chars}` token set at SessionStart, calls `SelectionItemPattern.Select()`

### Windows crate features needed
Both `src-tauri/Cargo.toml` and `hook-cli/Cargo.toml`:
```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_System_Threading",       # AttachThreadInput, GetCurrentThreadId, GetCurrentProcessId
    "Win32_System_Diagnostics_ToolHelp",  # CreateToolhelp32Snapshot
    "Win32_UI_WindowsAndMessaging", # HWND, EnumWindows, SetForegroundWindow, etc.
    "Win32_UI_Input_KeyboardAndMouse",    # SetFocus
    "Win32_System_Com",             # CoInitializeEx, CoCreateInstance
    "Win32_UI_Accessibility",       # IUIAutomation, UIA_TabItemControlTypeId, etc.
] }
```

## Pill UI states

| State | Window size | Trigger |
|-------|------------|---------|
| **Sliver** | 480×44px (only 14px visible) | Not hovered, no urgent session — pill pushed up via CSS transform |
| **Hover** | 480×44px | Mouse enters window (250ms debounce to collapse) |
| **Expanded + sessions** | 480×(44 + 8 + panel_height)px | Click or urgent session — panel slides in below pill |

- **Sliver**: `.root` has `transform: translateY(-30px)`. Only the bottom 14px of the pill peeks out.
- `isSliver = $derived(!isHovered && !userExpanded && !isAwaiting)` — no active-count condition.
- `urgentSession = $derived(activeSessions.find(s => s.pending_permission) ?? activeSessions.find(s => s.pending_question) ?? null)`
- `isAwaiting = $derived(urgentSession !== null)`
- `panelVariant = $derived(urgentSession?.pending_permission ? "code" : urgentSession?.pending_question ? "question" : "list")`
- Hover handlers are on `.hover-wrapper` (outer, no transform) NOT on `.root` (transformed).
- Easing: `cubic-bezier(0.2, 0, 0, 1)` everywhere (no spring).

## CSS constants (App.svelte)
- `WIN_W = 480` — window width (always)
- `PILL_H = 44` — pill height
- `SLIVER_OFFSET = 30` — translateY(-30) at rest → 14px visible
- `PANEL_GAP = 8` — gap between pill bottom and panel top
- Pill background `#0A0A0A`, border-radius `0 0 12px 12px`, padding `0 16px`
- Panel: `bg #0A0A0A, radius 18, shadow 0 12px 32px rgba(0,0,0,0.65)`

## Window / display

### Windows
- `alwaysOnTop: true, skipTaskbar: true, transparent: true, decorations: false` (tauri.conf.json)
- Tauri monitor APIs (`primary_monitor()` + `LogicalSize`/`LogicalPosition`) handle positioning
- No panel-detection needed (taskbar offset handled by Tauri on Windows)
- `kde_panel_thickness()` returns 0 on non-Linux platforms

### Linux
- `GDK_BACKEND=x11` in `main.rs` forces XWayland (never remove — GTK ignores `set_position` on Wayland)
- **Panel height**: read from `~/.config/plasmashellrc` → `thickness=28`. `_NET_WORKAREA` doesn't work on KDE Wayland + XWayland.
- Multi-monitor: external DP-2 (top, NOT primary), laptop eDP-1 (PRIMARY — pill goes here). Do NOT use "topmost by y" heuristic.

## AgentSession fields (state.rs)

```rust
pub struct AgentSession {
    pub session_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub phase: SessionPhase,
    pub summary: Option<String>,
    pub pending_permission: Option<PendingPermission>,
    pub pending_question: Option<String>,
    pub terminal_tty: Option<String>,        // Linux: path to controlling tty
    pub terminal_window_id: Option<String>,  // Windows: HWND as decimal string
    pub terminal_app: Option<String>,        // Windows: "WindowsTerminal.exe" etc.
    pub terminal_session_id: Option<String>, // Windows: WT_SESSION GUID
    pub terminal_pid: Option<String>,        // Windows: terminal host PID
    pub started_at: f64,
    pub updated_at: f64,
}
```

`copy_terminal_fields` in `server.rs` updates these from each payload (SessionStart + PreToolUse events). Only updates a field if the payload value is `Some`.

## Dot-glyph system (App.svelte)
- `TOOL_GLYPHS: Record<string, string>` — 12×12 grid strings (144 chars, '.' = empty, '#' = filled).
- `dotGlyph(key, size, color)` — returns inline SVG; use `{@html dotGlyph(...)}` in templates.
- `TOOL_GLYPH_MAP` — maps `Bash`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`, `WebFetch`, `WebSearch`, `Read` to glyph keys.
- Available keys: `bash`, `edit`, `write`, `multiedit`, `notebook`, `webfetch`, `websearch`, `read`, `check`, `alert`, `power`, `question`, `chevronDown`, `chevronUp`, `close`, `play`.
- Pulsing: `<span class:pulsing={condition}>` with `.pulsing` CSS opacity animation.

## Current state (as of 2026-05-24)

**Windows port is the active development target.**

- Sessions appear in the pill for real Claude Code sessions (hooks fire, bridge stores state).
- Hook auto-installs on every launch, survives plain "Quit".
- IPC via TCP works (hook reads port from `%APPDATA%/open-island/port`).
- Hook enriches payload with terminal HWND (`terminal_window_id`) + app name (`terminal_app`).
- Clicking a session row attempts `SetForegroundWindow` + UIA tab select via `OI-{8chars}` tab title token.
- AttachThreadInput fix applied (2026-05-23) to bypass overlay foreground restriction.
- UIA tab select uses `OI-{session_id[..8]}` token written at SessionStart via OSC-0 to CONOUT$.
- **Phase 1.5 IMPLEMENTED (2026-05-24) — UNTESTED**: Pill Allow/Deny buttons wired on Windows. Frontend has `handlePermission(allow, sessionId)` → invokes `resolve_permission` Tauri command → bridge resolves the oneshot channel → hook receives `ClaudeHookDirective` via TCP and returns `allow`/`deny` to Claude Code. Console keypress fallback (`y/n/1/2/Esc`) still works. **Requires real Claude Code session to verify end-to-end.**
- **hook-cli compilation fixed (2026-05-24)**: Fixed `ConsoleModeGuard` type mismatches with `windows` crate 0.58 (`CONSOLE_MODE` vs raw `u32`), removed duplicate `impl` blocks, fixed `HANDLE` `Send` issue in `spawn_keypress_reader` by passing raw `isize` handle value to the thread closure.
- **Sliver mode window sizing fixed (2026-05-24)**: Window now dynamically shrinks to match the actual pill dimensions in sliver mode — **14px height** (was 44px) and **pill-width + 8px** (was 480px). This eliminates transparent dead space that was blocking clicks and triggering false hovers. Uses `ResizeObserver` on the `.pill` element + `set_window_geometry` to resize and recenter. **Tested visually — works.**
- **Clean sliver mode (2026-05-24)**: In sliver mode, badges, agent count, and chevron are now completely hidden — only the clean black pill background shows. This prevents the purple tool badge from bleeding outside the pill bounds.
- **Sliver mode window sizing fixed (2026-05-24)**: Window now dynamically shrinks to match the actual pill dimensions in sliver mode — **14px height** (was 44px) and **pill-width + 8px** (was 480px). This eliminates transparent dead space that was blocking clicks and triggering false hovers. Uses `ResizeObserver` on the `.pill` element + `set_window_geometry` to resize and recenter. **Tested visually — works.**
- **Pending / deferred**: ConPTY-conhost edge case for sessions without HWND.
- **Pending / deferred**: ConPTY-conhost edge case for sessions without HWND.

**Linux** (lower priority, still works):
- Sliver/hover/expand/session-list/permission flow all working.
- Pill positions correctly on eDP-1 below 28px KDE panel.

## Known issues / gotchas

### Windows
- `SetForegroundWindow` silently fails from overlay windows without `AttachThreadInput` — Windows flashes the taskbar instead of activating the target window.
- `AttachThreadInput` is in `windows::Win32::System::Threading` (NOT `Win32::UI::WindowsAndMessaging` as MSDN placement might suggest).
- `SetFocus` is in `windows::Win32::UI::Input::KeyboardAndMouse`.
- Shells (`powershell.exe`, `cmd.exe`) must NOT be in TERMINAL_APPS — they have no window; the walk must continue past them to find `WindowsTerminal.exe`.
- `conhost.exe` found via the fallback (child of shell) may be a ConPTY with no visible window → `hwnd_for_pid` returns None; those sessions have no HWND and click is a no-op.
- CC reads `settings.json` once at startup — hooks only apply to new CC sessions after hooks are installed.
- Hook log `%TEMP%\oi-hook-log.txt` is a debug artifact — remove before release.

### Linux
- `_NET_WORKAREA` returns y=0 on KDE Wayland + XWayland — useless for panel detection.
- `current_monitor()` returns None before WM maps the window — use `primary_monitor()`.
- `set_size(PhysicalSize(w, h))` on a 2x display creates a half-logical-pixel window — always use `LogicalSize`.
- XWayland doubles X11 coordinates in wmctrl output (visual position is still correct — do NOT halve).
- `set_window_geometry` spawns a delayed 80ms retry — do not call in a tight loop.
- `.panel-clip` must NOT animate `max-height` (breaks ResizeObserver in WebKit2GTK) — use only `opacity` + `transform`.

## Svelte 5 notes
- Uses rune API: `$state`, `$derived`, `$effect`
- `mount()` from `svelte` (not `new App()`)
- `$effect` runs before `onMount` in the first render cycle
- Effects CAN set `$state` variables without loops if the change doesn't re-trigger the effect's dependencies

## Windows port — remaining work

- **Installer**: NSIS installer via `cargo tauri build` (already configured in tauri.conf.json).
- **Taskbar positioning**: Currently using `primary_monitor()` — confirm pill lands just below Windows taskbar.

## Development environment notes (Windows)

- **PATH requirements**: Must have `C:\Users\wilan\.cargo\bin` (for cargo) and `C:\Users\wilan\AppData\Roaming\npm` (for pnpm) in PATH.
- **pnpm/node_modules issues**: If `pnpm dev` fails with "Cannot find module", dependencies may need reinstall (`rm -rf node_modules && pnpm install`).
- **Workaround for dev server issues**: If `cargo tauri dev` fails due to missing pnpm/node, use `--no-dev-server` flag with a pre-built `dist` folder, or temporarily change `beforeDevCommand` in `tauri.conf.json` to `"echo skip"`.
- **Alternative dev server**: Can serve `dist/` folder on port 5173 with `npx serve -l 5173` or Python http.server.
- **tauri.conf.json changes made 2026-05-24**: `beforeDevCommand` and `beforeBuildCommand` temporarily changed to `"echo skip"` to bypass pnpm issues during development.
