use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::bridge::server::BridgeServer;

fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("settings.json")
}

fn hook_binary_path() -> String {
    // Look for hook-cli next to the app binary, or in PATH
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.parent().unwrap_or(std::path::Path::new("/usr/local/bin"))
            .join("open-island-hook");
        if sibling.exists() {
            return sibling.to_string_lossy().to_string();
        }
    }
    "open-island-hook".to_string()
}

fn socket_path_str() -> String {
    BridgeServer::socket_path().to_string_lossy().to_string()
}

fn make_hook_entry(event: &str) -> Value {
    // Claude Code format: { matcher: "", hooks: [{ type: "command", command: "..." }] }
    json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": format!(
                "OPEN_ISLAND_SOCKET_PATH={} {} {}",
                socket_path_str(),
                hook_binary_path(),
                event
            )
        }]
    })
}

pub fn install() -> Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut settings: Value = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Reading {:?}", path))?;
        serde_json::from_str(&raw).unwrap_or(json!({}))
    } else {
        json!({})
    };

    let hook_events = [
        "SessionStart",
        "SessionEnd",
        "PreToolUse",
        "PostToolUse",
        "Stop",
        "Notification",
    ];

    if settings["hooks"].is_null() || !settings["hooks"].is_object() {
        settings["hooks"] = json!({});
    }

    for event in &hook_events {
        let entry = make_hook_entry(event);
        let arr = settings["hooks"][event].as_array_mut();
        if let Some(arr) = arr {
            // Remove stale open-island entries then add fresh one
            arr.retain(|v| {
                !v["hooks"].as_array()
                    .and_then(|h| h.first())
                    .and_then(|h| h["command"].as_str())
                    .map(|s| s.contains("open-island-hook"))
                    .unwrap_or(false)
            });
            arr.push(entry);
        } else {
            settings["hooks"][event] = json!([entry]);
        }
    }

    let out = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&path, out)
        .with_context(|| format!("Writing {:?}", path))?;

    log::info!("Claude hooks installed to {:?}", path);
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let path = settings_path();
    if !path.exists() {
        return Ok(());
    }

    let raw = std::fs::read_to_string(&path)?;
    let mut settings: Value = serde_json::from_str(&raw).unwrap_or(json!({}));

    if let Some(hooks) = settings["hooks"].as_object_mut() {
        for (_, arr) in hooks.iter_mut() {
            if let Some(arr) = arr.as_array_mut() {
                arr.retain(|v| {
                    !v["hooks"].as_array()
                        .and_then(|h| h.first())
                        .and_then(|h| h["command"].as_str())
                        .map(|s| s.contains("open-island-hook"))
                        .unwrap_or(false)
                });
            }
        }
    }

    let out = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&path, out)?;
    log::info!("Claude hooks removed from {:?}", path);
    Ok(())
}
