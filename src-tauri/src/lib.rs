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

pub struct AppState {
    pub bridge: Arc<Mutex<BridgeServer>>,
}

#[tauri::command]
async fn get_sessions(state: tauri::State<'_, AppState>) -> Result<Vec<AgentSession>, String> {
    let bridge = state.bridge.lock().await;
    Ok(bridge.sessions_snapshot().await)
}

#[tauri::command]
async fn resolve_permission(
    session_id: String,
    allow: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let resolution = if allow {
        PermissionResolution::Allow
    } else {
        PermissionResolution::Deny { message: Some("Denied by user".to_string()) }
    };
    let bridge = state.bridge.lock().await;
    bridge.resolve_permission(session_id, resolution).await;
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
    BridgeServer::socket_path().to_string_lossy().to_string()
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// The monitor the pill lives on: the one with the smallest global y-position
/// (topmost in the combined desktop), which is where the KDE top panel lives.
/// Falls back to primary → current monitor.
fn top_monitor(win: &tauri::WebviewWindow) -> Option<tauri::Monitor> {
    win.available_monitors()
        .ok()
        .and_then(|mut ms| {
            if ms.is_empty() { return None; }
            ms.sort_by_key(|m| m.position().y);
            ms.into_iter().next()
        })
        .or_else(|| win.primary_monitor().ok().flatten())
        .or_else(|| win.current_monitor().ok().flatten())
}

/// Read the KDE panel thickness from plasmashellrc (KDE Wayland doesn't expose
/// it via _NET_WORKAREA for XWayland clients).
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

/// Returns (x, y) in logical px: centered on the primary monitor, just below its panel.
fn primary_top_center(win: &tauri::WebviewWindow, width: u32) -> Option<(f64, f64)> {
    let m = win.primary_monitor().ok().flatten()
        .or_else(|| win.current_monitor().ok().flatten())?;
    let scale = m.scale_factor();
    let mon_x = m.position().x as f64 / scale;
    let mon_y = m.position().y as f64 / scale;
    let mon_w = m.size().width as f64 / scale;
    let x = (mon_x + (mon_w - width as f64) / 2.0).max(mon_x);
    let panel_h = kde_panel_thickness() as f64; // already in X11/logical px (scale≈1)
    let y = mon_y + panel_h;
    log::info!("primary_top_center: monitor={:?} scale={scale} x={x} y={y} panel_h={panel_h}", m.name());
    Some((x, y))
}

fn position_at_top(win: &tauri::WebviewWindow) {
    let win_w = win.outer_size()
        .map(|s| {
            let scale = win.scale_factor().unwrap_or(1.0);
            (s.width as f64 / scale).round() as u32
        })
        .unwrap_or(200);
    if let Some((x, y)) = primary_top_center(win, win_w) {
        log::info!("position_at_top: win_w={win_w} → x={x} y={y}");
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

fn build_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show / Hide", true, None::<&str>)?;
    let install = MenuItem::with_id(app, "install_hooks", "Install Claude Hooks", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Open Island", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &install, &quit])?;

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
