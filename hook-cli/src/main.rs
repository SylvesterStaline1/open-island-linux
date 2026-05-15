/// Claude Code hook relay — reads JSON payload from stdin, forwards to the
/// Open Island bridge via Unix socket, and writes the directive to stdout.
/// Exits silently if the bridge is unreachable (fail-open behaviour).
use std::io::{self, BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("OPEN_ISLAND_SOCKET_PATH") {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("VIBE_ISLAND_SOCKET_PATH") {
        return PathBuf::from(p);
    }
    // Mirror the default from the server
    if let Some(runtime) = runtime_dir() {
        return runtime.join("open-island").join("bridge.sock");
    }
    PathBuf::from("/tmp/open-island-bridge.sock")
}

fn runtime_dir() -> Option<PathBuf> {
    // XDG_RUNTIME_DIR is the canonical place on Linux
    std::env::var("XDG_RUNTIME_DIR").ok().map(PathBuf::from)
}

fn main() {
    // Read the full stdin payload
    let mut payload = String::new();
    if io::stdin().read_to_string(&mut payload).is_err() {
        return;
    }
    let payload = payload.trim().to_string();
    if payload.is_empty() {
        return;
    }

    // Parse it just enough to get the session_id and hook_event_name
    let parsed: serde_json::Value = match serde_json::from_str(&payload) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Wrap in a BridgeEnvelope { type: "command", command: { type: "processClaudeHook", claudeHook: ... } }
    let envelope = serde_json::json!({
        "type": "command",
        "command": {
            "type": "processClaudeHook",
            "claudeHook": parsed
        }
    });

    let path = socket_path();
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => return, // bridge not running — fail open
    };

    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let msg = match serde_json::to_string(&envelope) {
        Ok(s) => s,
        Err(_) => return,
    };

    if stream.write_all(msg.as_bytes()).is_err() {
        return;
    }
    if stream.write_all(b"\n").is_err() {
        return;
    }

    // Read response lines until we get a claudeHookDirective (or EOF)
    let reader = io::BufReader::new(stream);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let env: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if env["type"] == "response" {
            let resp_type = env["response"]["type"].as_str().unwrap_or("");
            if resp_type == "claudeHookDirective" {
                let directive = &env["response"]["directive"];
                if directive["type"] == "deny" {
                    // Write the deny response to stdout so Claude Code sees it
                    let out = serde_json::json!({
                        "decision": "block",
                        "reason": directive["message"].as_str().unwrap_or("Denied by Open Island")
                    });
                    let _ = io::stdout().write_all(serde_json::to_string(&out).unwrap().as_bytes());
                }
                // Allow = write nothing (Claude Code continues)
                break;
            } else if resp_type == "acknowledged" {
                break;
            }
        }
    }
}
