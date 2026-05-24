#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Force X11 backend so GTK honours set_position on KDE Wayland (via XWayland).
    // Linux-only: GDK/GTK does not exist on Windows (Tauri uses WebView2 there).
    #[cfg(target_os = "linux")]
    std::env::set_var("GDK_BACKEND", "x11");
    open_island_lib::run();
}
