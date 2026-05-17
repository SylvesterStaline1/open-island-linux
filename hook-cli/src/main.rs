/// Claude Code hook relay — forwards hook payloads to the Open Island bridge socket,
/// then for gated tools blocks waiting for a user decision from EITHER:
///   1. A keypress on /dev/tty (y/n in the terminal where CC is running), OR
///   2. A directive pushed back over the bridge socket when the user clicks the pill.
///
/// Whichever fires first wins. Returns allow/deny (or "ask" on timeout/error so CC
/// falls back to its own native prompt). No ydotool, no wmctrl, no global keyboard
/// injection — works out-of-the-box on any Linux desktop.
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("OPEN_ISLAND_SOCKET_PATH") {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("VIBE_ISLAND_SOCKET_PATH") {
        return PathBuf::from(p);
    }
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("open-island").join("bridge.sock");
    }
    PathBuf::from("/tmp/open-island-bridge.sock")
}

fn requires_approval(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "Bash" | "Edit" | "Write" | "MultiEdit"
            | "NotebookEdit" | "WebFetch" | "WebSearch" | "computer_use"
    )
}

#[allow(dead_code)]
fn fmt_input(input: &serde_json::Value) -> String {
    if input.is_null() {
        return String::new();
    }
    let s = input.to_string();
    if s.len() > 60 {
        format!("{}…", &s[..57])
    } else {
        s
    }
}

// ── Retained for Windows port ─────────────────────────────────────────────────
// On Windows, WriteConsoleInput lets the bridge inject "1\n"/"2\n" into CC's
// console unprivileged. The Windows hook will call wait_for_decision() instead
// of immediately returning "ask" — see project_open_island_crossplatform memory.
#[allow(dead_code)]
enum Decision {
    Allow,
    Deny,
    Fallback,
}

/// RAII guard: puts fd into cbreak/no-echo mode and restores on drop.
/// If setup fails (e.g. fd is not a tty), returns None and does nothing.
#[allow(dead_code)]
struct RawModeGuard {
    fd: i32,
    saved: libc::termios,
}

impl RawModeGuard {
    fn new(fd: i32) -> Option<Self> {
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
            return None;
        }
        let mut raw = saved;
        raw.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON);
        raw.c_cc[libc::VMIN as usize] = 1;
        raw.c_cc[libc::VTIME as usize] = 0;
        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) } != 0 {
            return None;
        }
        Some(Self { fd, saved })
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.saved); }
    }
}

/// Poll /dev/tty (for keyboard input) and the bridge socket (for pill decision)
/// simultaneously. Returns the first decision that arrives, or Fallback after 30s.
#[allow(dead_code)]
fn wait_for_decision(
    socket_fd: Option<i32>,
    tool_name: &str,
    tool_input: &serde_json::Value,
) -> Decision {
    let deadline = Instant::now() + Duration::from_secs(30);

    // Open /dev/tty so we can show a prompt and read single keystrokes,
    // even if stdin/stdout are redirected (as CC does for the hook process).
    let tty = OpenOptions::new().read(true).write(true).open("/dev/tty").ok();
    let tty_fd = tty.as_ref().map(|f| f.as_raw_fd());

    if let Some(fd) = tty_fd {
        let input_str = fmt_input(tool_input);
        let prompt = if input_str.is_empty() {
            format!(
                "\r\n\x1b[1;37m● Open Island\x1b[0m  {}\r\n  \x1b[2my\x1b[0m=allow  \x1b[2mn\x1b[0m=deny  (or click the pill)\r\n",
                tool_name
            )
        } else {
            format!(
                "\r\n\x1b[1;37m● Open Island\x1b[0m  {}  \x1b[2m{}\x1b[0m\r\n  \x1b[2my\x1b[0m=allow  \x1b[2mn\x1b[0m=deny  (or click the pill)\r\n",
                tool_name, input_str
            )
        };
        unsafe { libc::write(fd, prompt.as_ptr() as *const libc::c_void, prompt.len()); }
    }

    // cbreak + no-echo for instant single-key reads; Drop guard restores on exit
    let _raw = tty_fd.and_then(RawModeGuard::new);

    if socket_fd.is_none() && tty_fd.is_none() {
        return Decision::Fallback;
    }

    let mut socket_line: Vec<u8> = Vec::new();

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Decision::Fallback;
        }
        let timeout_ms = remaining.as_millis().min(5_000) as libc::c_int;

        let mut pfds: Vec<libc::pollfd> = Vec::new();
        if let Some(fd) = tty_fd {
            pfds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
        }
        if let Some(fd) = socket_fd {
            pfds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
        }

        let ret = unsafe {
            libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout_ms)
        };
        if ret <= 0 {
            continue;
        }

        for pfd in &pfds {
            if pfd.revents & libc::POLLIN != 0 {
                if Some(pfd.fd) == tty_fd {
                    // Read one byte from terminal
                    let mut ch: libc::c_char = 0;
                    let n = unsafe {
                        libc::read(pfd.fd, &mut ch as *mut libc::c_char as *mut libc::c_void, 1)
                    };
                    if n == 1 {
                        match ch as u8 {
                            b'y' | b'Y' | b'1' => return Decision::Allow,
                            b'n' | b'N' | b'2' | 3 | 27 => return Decision::Deny, // n, Ctrl-C, Esc
                            _ => {}
                        }
                    }
                } else if Some(pfd.fd) == socket_fd {
                    // Read one byte from socket; accumulate until newline
                    let mut byte = 0u8;
                    let n = unsafe {
                        libc::read(pfd.fd, &mut byte as *mut u8 as *mut libc::c_void, 1)
                    };
                    if n <= 0 {
                        return Decision::Fallback; // Connection closed
                    }
                    if byte == b'\n' {
                        // Parse the BridgeEnvelope::Response::ClaudeHookDirective
                        if let Ok(s) = std::str::from_utf8(&socket_line) {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
                                match val["response"]["directive"]["type"].as_str() {
                                    Some("allow") => return Decision::Allow,
                                    Some("deny") => return Decision::Deny,
                                    _ => {}
                                }
                            }
                        }
                        socket_line.clear();
                    } else {
                        socket_line.push(byte);
                    }
                }
            }
            if pfd.revents & (libc::POLLHUP | libc::POLLERR) != 0
                && Some(pfd.fd) == socket_fd
            {
                return Decision::Fallback;
            }
        }
    }
}

fn main() {
    let mut payload = String::new();
    if io::stdin().read_to_string(&mut payload).is_err() {
        return;
    }
    let payload = payload.trim().to_string();
    if payload.is_empty() {
        return;
    }

    let parsed: serde_json::Value = match serde_json::from_str(&payload) {
        Ok(v) => v,
        Err(_) => return,
    };

    let event = parsed["hook_event_name"].as_str().unwrap_or("").to_lowercase();
    let tool_name = parsed["tool_name"].as_str().unwrap_or("").to_string();
    let needs_permission = (event == "pretooluse" || event == "permissionrequest")
        && requires_approval(&tool_name);

    let envelope = serde_json::json!({
        "type": "command",
        "command": {
            "type": "processClaudeHook",
            "claudeHook": parsed
        }
    });

    // Forward to bridge and wait for ack; if bridge is not running, proceed silently.
    if let Ok(mut s) = UnixStream::connect(socket_path()) {
        if let Ok(msg) = serde_json::to_string(&envelope) {
            s.set_write_timeout(Some(Duration::from_secs(3))).ok();
            s.set_read_timeout(Some(Duration::from_secs(3))).ok();
            let sent = s.write_all(msg.as_bytes()).is_ok() && s.write_all(b"\n").is_ok();
            if sent {
                let mut ack_buf = [0u8; 256];
                let _ = s.read(&mut ack_buf);
                // Windows port: store `s` and pass s.as_raw_fd() into wait_for_decision()
                // so the bridge can push back a ClaudeHookDirective after pill click.
            }
        }
    }

    if needs_permission {
        // Return "ask" so CC shows its own native `1. Yes  2. No` prompt in the TUI,
        // which is visible and safe. The pill notification is informational + click-to-focus.
        // Windows port: replace this block with wait_for_decision() so the pill's
        // Allow/Deny buttons (using WriteConsoleInput) can resolve the prompt directly.
        let out = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "ask"
            }
        });
        let _ = io::stdout().write_all(serde_json::to_string(&out).unwrap().as_bytes());
    }
}
