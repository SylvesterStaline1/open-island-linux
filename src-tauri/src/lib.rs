mod bridge;
mod hooks;

use std::sync::Arc;

use bridge::{BridgeServer, PermissionResolution};
use bridge::server::ServerEvent;
use bridge::state::AgentSession;
use tauri::{
    AppHandle, Emitter, Manager,
    tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState},
    menu::{Menu, MenuItem},
};
use tokio::sync::Mutex;
use tauri::async_runtime;

/// Logical pixel width of the pill window. Must match `tauri.conf.json` `width`
/// and `WIN_W` in `src/App.svelte`.
const PILL_WIDTH: u32 = 480;

pub struct AppState {
    pub bridge: Arc<Mutex<BridgeServer>>,
}

#[tauri::command]
async fn get_sessions(state: tauri::State<'_, AppState>) -> Result<Vec<AgentSession>, String> {
    let bridge = state.bridge.lock().await;
    Ok(bridge.sessions_snapshot().await)
}

#[tauri::command(rename_all = "snake_case")]
async fn resolve_permission(
    session_id: String,
    allow: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    log::info!("resolve_permission: session_id={session_id} allow={allow}");
    let resolution = if allow {
        PermissionResolution::Allow
    } else {
        PermissionResolution::Deny { message: Some("Denied by user".to_string()) }
    };
    let bridge = state.bridge.lock().await;
    bridge.resolve_permission(session_id, resolution).await;
    log::info!("resolve_permission: done");
    Ok(())
}

#[tauri::command]
async fn install_hooks() -> Result<(), String> {
    hooks::claude::install().map_err(|e| e.to_string())
}

#[tauri::command]
async fn uninstall_hooks() -> Result<(), String> {
    hooks::claude::uninstall().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_socket_path() -> String {
    BridgeServer::port_file_path().to_string_lossy().to_string()
}

#[tauri::command(rename_all = "snake_case")]
async fn focus_session_terminal(session_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let bridge = state.bridge.lock().await;
    let sessions = bridge.sessions_snapshot().await;
    let session = sessions.into_iter().find(|s| s.session_id == session_id);
    drop(bridge);

    let Some(session) = session else { return Ok(()) };

    std::thread::spawn(move || {
        focus_session(&session);
    });

    Ok(())
}

#[tauri::command]
async fn set_window_size(width: u32, height: u32, app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(width as f64, height as f64)))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn set_window_geometry(width: u32, height: u32, app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(width as f64, height as f64)))
            .map_err(|e| e.to_string())?;

        if let Some((x, y)) = primary_top_center(&win, width) {
            log::info!("set_window_geometry: w={width} → x={x} y={y}");
            let _ = win.set_position(tauri::LogicalPosition::new(x, y));
            // Retry — KDE/XWayland may reposition after resize
            let app2 = app.clone();
            async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
                if let Some(w) = app2.get_webview_window("main") {
                    let _ = w.set_position(tauri::LogicalPosition::new(x, y));
                }
            });
        }
    }
    Ok(())
}

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let mut bridge = BridgeServer::new();
            let event_rx = bridge.event_rx.take().expect("event_rx taken twice");

            async_runtime::block_on(async {
                bridge.start().await.expect("Bridge server failed to start");
            });

            let bridge = Arc::new(Mutex::new(bridge));
            app.manage(AppState { bridge: bridge.clone() });

            // Auto-install hooks so users don't need to click "Install Claude Hooks"
            // after every launch. Idempotent — safe to call every startup.
            if let Err(e) = hooks::claude::install() {
                log::warn!("Auto-install of Claude hooks failed: {e}");
            }

            let app_handle = app.handle().clone();
            async_runtime::spawn(forward_events(event_rx, app_handle));

            build_tray(app)?;

            // Fix y=0 (keep WM-centered x from "center: true"), then show
            if let Some(win) = app.get_webview_window("main") {
                position_at_top(&win);
                let _ = win.show();
            }

            // Retry after WM maps the window (XWayland may ignore pre-map set_position)
            let app_handle2 = app.handle().clone();
            async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                if let Some(win) = app_handle2.get_webview_window("main") {
                    position_at_top(&win);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_sessions,
            resolve_permission,
            install_hooks,
            uninstall_hooks,
            get_socket_path,
            set_window_size,
            set_window_geometry,
            focus_session_terminal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Read the KDE panel thickness from plasmashellrc (KDE Wayland doesn't expose
/// it via _NET_WORKAREA for XWayland clients). Linux-only; returns 0 elsewhere
/// since Tauri's monitor APIs already account for the taskbar on Windows.
#[cfg(target_os = "linux")]
fn kde_panel_thickness() -> i32 {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = std::path::Path::new(&home).join(".config/plasmashellrc");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.trim_start().starts_with("thickness="))
                .and_then(|l| l.split('=').nth(1))
                .and_then(|v| v.trim().parse::<i32>().ok())
        })
        .unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
fn kde_panel_thickness() -> i32 {
    0
}

/// Returns (x, y) in logical px: centered on the primary monitor, just below its panel.
fn primary_top_center(win: &tauri::WebviewWindow, width: u32) -> Option<(f64, f64)> {
    let m = win.primary_monitor().ok().flatten()
        .or_else(|| win.current_monitor().ok().flatten())?;
    let scale = m.scale_factor();
    let mon_x = m.position().x as f64 / scale;
    let mon_y = m.position().y as f64 / scale;
    let mon_w = m.size().width as f64 / scale;
    let x = (mon_x + (mon_w - width as f64) / 2.0).max(mon_x);
    let panel_h = kde_panel_thickness() as f64;
    let y = mon_y + panel_h;
    log::info!("primary_top_center: monitor={:?} scale={scale} x={x} y={y} panel_h={panel_h}", m.name());
    Some((x, y))
}

fn position_at_top(win: &tauri::WebviewWindow) {
    // outer_size() is unreliable before win.show() on XWayland — use the
    // known constant instead of querying the not-yet-mapped window.
    if let Some((x, y)) = primary_top_center(win, PILL_WIDTH) {
        log::info!("position_at_top: win_w={PILL_WIDTH} → x={x} y={y}");
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    } else {
        log::warn!("position_at_top: primary monitor not found");
    }
}



async fn forward_events(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<ServerEvent>,
    app: AppHandle,
) {
    while let Some(event) = rx.recv().await {
        match event {
            ServerEvent::SessionsChanged(sessions) => {
                let _ = app.emit("sessions-changed", &sessions);
                update_tray_tooltip(&app, &sessions);
            }
            ServerEvent::PermissionRequested { session_id, tool_name, tool_input } => {
                let _ = app.emit("permission-requested", serde_json::json!({
                    "sessionId": session_id,
                    "toolName": tool_name,
                    "toolInput": tool_input,
                }));
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_focus();
                }
            }
            ServerEvent::PermissionResolved(session_id) => {
                let _ = app.emit("permission-resolved", &session_id);
            }
            ServerEvent::Notification { title, message } => {
                let _ = app.emit("agent-notification", serde_json::json!({
                    "title": title,
                    "message": message,
                }));
            }
        }
    }
}

fn update_tray_tooltip(app: &AppHandle, sessions: &[bridge::state::AgentSession]) {
    let active = sessions.iter().filter(|s| {
        s.phase != bridge::state::SessionPhase::Completed
    }).count();

    let tooltip = if active == 0 {
        "Open Island — no active agents".to_string()
    } else {
        format!("Open Island — {} agent{} running", active, if active == 1 { "" } else { "s" })
    };

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(&tooltip));
    }
}

fn focus_session(session: &AgentSession) {
    #[cfg(target_os = "linux")]
    if let Some(tty) = &session.terminal_tty {
        if let Some(pid) = find_terminal_pid_for_tty(tty) {
            focus_window_by_pid(pid);
        }
        return;
    }

    #[cfg(windows)]
    focus_session_windows(session);

    let _ = session; // suppress unused-variable warning on other platforms
}

#[cfg(windows)]
fn focus_session_windows(session: &AgentSession) {
    log::info!(
        "focus_session_windows: hwnd={:?} app={:?} sid={}",
        session.terminal_window_id,
        session.terminal_app,
        &session.session_id,
    );
    let Some(hwnd_str) = &session.terminal_window_id else {
        log::warn!("focus_session_windows: no terminal_window_id — nothing to focus");
        return;
    };
    let Ok(hwnd_val) = hwnd_str.parse::<isize>() else {
        log::warn!("focus_session_windows: invalid hwnd '{hwnd_str}'");
        return;
    };

    use windows::Win32::Foundation::{BOOL, HWND};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
        IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let hwnd = HWND(hwnd_val as *mut core::ffi::c_void);
    unsafe {
        // AttachThreadInput trick: bypass Windows' foreground-window restriction.
        // Overlay windows (alwaysOnTop + skipTaskbar) are not granted foreground
        // rights by Windows, so plain SetForegroundWindow silently fails and only
        // flashes the taskbar. Attaching our thread's input queue to both the
        // current foreground thread and the target thread makes us share the same
        // input context, which satisfies the restriction.
        let target_tid = GetWindowThreadProcessId(hwnd, None);
        let fg_tid     = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let our_tid    = GetCurrentThreadId();

        let a1 = fg_tid != 0 && fg_tid != our_tid;
        let a2 = target_tid != 0 && target_tid != our_tid && target_tid != fg_tid;
        if a1 { let _ = AttachThreadInput(our_tid, fg_tid,     BOOL(1)); }
        if a2 { let _ = AttachThreadInput(our_tid, target_tid, BOOL(1)); }

        if IsIconic(hwnd).as_bool() { let _ = ShowWindow(hwnd, SW_RESTORE); }
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);

        if a2 { let _ = AttachThreadInput(our_tid, target_tid, BOOL(0)); }
        if a1 { let _ = AttachThreadInput(our_tid, fg_tid,     BOOL(0)); }
    }
    log::info!("focus_session_windows: focus dispatched for hwnd={hwnd_val}");

    if session.terminal_app.as_deref() == Some("WindowsTerminal.exe") {
        let token = format!("OI-{}", &session.session_id[..session.session_id.len().min(8)]);
        if let Err(e) = select_wt_tab_by_title(&token, hwnd_val) {
            log::debug!("UIA tab select: {e}");
        }
    }
}

#[cfg(windows)]
fn select_wt_tab_by_title(token: &str, hwnd_val: isize) -> windows::core::Result<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation8, IUIAutomation, IUIAutomationSelectionItemPattern,
        TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_SelectionItemPatternId,
        UIA_TabItemControlTypeId,
    };
    use windows::core::{Interface, VARIANT};

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;

        let hwnd = HWND(hwnd_val as *mut core::ffi::c_void);
        let root = automation.ElementFromHandle(hwnd)?;

        let variant = VARIANT::from(UIA_TabItemControlTypeId.0 as i32);
        let condition =
            automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &variant)?;

        let elements = root.FindAll(TreeScope_Descendants, &condition)?;
        let count = elements.Length()?;

        for i in 0..count {
            let elem = elements.GetElement(i)?;
            let name = elem.CurrentName()?.to_string();
            if name.contains(token) {
                let pattern: IUIAutomationSelectionItemPattern =
                    elem.GetCurrentPattern(UIA_SelectionItemPatternId)?.cast()?;
                pattern.Select()?;
                log::debug!("UIA: activated tab '{name}'");
                return Ok(());
            }
        }

        log::debug!("UIA: no tab found containing '{token}' (tab title may have been overwritten)");
        Ok(())
    }
}

/// Walk /proc to find a terminal emulator whose child has the given controlling tty.
#[cfg(target_os = "linux")]
fn find_terminal_pid_for_tty(tty_path: &str) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;

    let tty_dev = std::fs::metadata(tty_path).ok()?.rdev();

    // Collect PIDs whose controlling tty matches.
    let mut tty_pids: Vec<u32> = Vec::new();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
        // /proc/pid/stat: "pid (comm) state ppid pgrp session tty_nr ..."
        // comm may contain spaces/parens; rfind(')') reliably finds the end.
        let Some(after) = stat.rfind(')').map(|i| &stat[i + 2..]) else { continue };
        let fields: Vec<&str> = after.split_whitespace().collect();
        if fields.len() < 5 { continue }
        // field[4] = tty_nr as a signed decimal matching the kernel dev_t encoding
        let Ok(tty_nr) = fields[4].parse::<i64>() else { continue };
        if tty_nr > 0 && tty_nr as u64 == tty_dev {
            tty_pids.push(pid);
        }
    }

    const TERMINALS: &[&str] = &[
        "konsole", "gnome-terminal", "xterm", "alacritty",
        "kitty", "wezterm", "foot", "tilix", "terminator",
        "urxvt", "rxvt", "xfce4-terminal", "lxterminal",
        "mate-terminal", "st", "sakura",
    ];

    // For each candidate PID, walk up the parent chain looking for a terminal emulator.
    for pid in tty_pids {
        let mut cur = pid;
        for _ in 0..15 {
            let comm = std::fs::read_to_string(format!("/proc/{cur}/comm"))
                .unwrap_or_default();
            let comm = comm.trim();
            if TERMINALS.iter().any(|&t| comm == t || comm.starts_with(t)) {
                return Some(cur);
            }
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{cur}/stat")) else { break };
            let Some(after) = stat.rfind(')').map(|i| &stat[i + 2..]) else { break };
            let ppid: u32 = after.split_whitespace().nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if ppid == 0 || ppid == cur { break }
            cur = ppid;
        }
    }

    None
}

/// Focus a window by PID using wmctrl (preferred) or xdotool (fallback).
/// Both are optional; graceful no-op if neither is installed.
#[cfg(target_os = "linux")]
fn focus_window_by_pid(pid: u32) {
    if let Ok(out) = std::process::Command::new("wmctrl").arg("-lp").output() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() >= 3 && cols[2].parse::<u32>().ok() == Some(pid) {
                let _ = std::process::Command::new("wmctrl")
                    .args(["-i", "-a", cols[0]])
                    .status();
                return;
            }
        }
    }
    // wmctrl not found or window not listed — try xdotool
    let _ = std::process::Command::new("xdotool")
        .args(["search", "--pid", &pid.to_string(), "windowactivate", "--sync"])
        .status();
}

fn build_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show / Hide", true, None::<&str>)?;
    let install = MenuItem::with_id(app, "install_hooks", "Install Claude Hooks", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Open Island", true, None::<&str>)?;
    let uninstall_quit = MenuItem::with_id(app, "uninstall_quit", "Uninstall Hooks & Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &install, &quit, &uninstall_quit])?;

    TrayIconBuilder::with_id("main")
        .tooltip("Open Island")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(win) = app.get_webview_window("main") {
                    if win.is_visible().unwrap_or(false) {
                        let _ = win.hide();
                    } else {
                        position_at_top(&win);
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
            }
            "install_hooks" => {
                if let Err(e) = hooks::claude::install() {
                    log::error!("Hook install failed: {}", e);
                }
            }
            "quit" => {
                app.exit(0);
            }
            "uninstall_quit" => {
                let _ = hooks::claude::uninstall();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("main") {
                    if win.is_visible().unwrap_or(false) {
                        let _ = win.hide();
                    } else {
                        position_at_top(&win);
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}
