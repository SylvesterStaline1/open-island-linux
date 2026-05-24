/// Claude Code hook relay — forwards hook payloads to the Open Island bridge over
/// TCP localhost, then for gated tools waits for either a console keypress or a
/// pill-side directive before returning the permission decision to Claude Code.
///
/// No ydotool, no wmctrl, no global keyboard injection — works out-of-the-box on
/// any desktop (Linux or Windows).
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(unix)]
use std::time::Instant;

/// Read the TCP port that the bridge server wrote on startup.
/// Path: ~/.config/open-island/port  (Linux)  or  %APPDATA%/open-island/port  (Windows)
fn read_port() -> Option<u16> {
    let path = dirs::config_dir()?.join("open-island").join("port");
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn requires_approval(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "Bash"
            | "Edit"
            | "Write"
            | "MultiEdit"
            | "NotebookEdit"
            | "WebFetch"
            | "WebSearch"
            | "computer_use"
    )
}

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

enum Decision {
    Allow,
    Deny,
    Fallback,
}

// ── Unix-only: cbreak terminal mode and /dev/tty polling ──────────────────────

/// RAII guard: puts fd into cbreak/no-echo mode and restores on drop.
#[cfg(unix)]
struct RawModeGuard {
    fd: i32,
    saved: libc::termios,
}

#[cfg(unix)]
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

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.saved);
        }
    }
}

/// Poll /dev/tty (for keyboard input) and the bridge socket (for pill decision)
/// simultaneously. Returns the first decision that arrives, or Fallback after 30s.
#[cfg(unix)]
fn wait_for_decision(
    socket_fd: Option<i32>,
    tool_name: &str,
    tool_input: &serde_json::Value,
) -> Decision {
    let deadline = Instant::now() + Duration::from_secs(30);

    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok();
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
        unsafe {
            libc::write(fd, prompt.as_ptr() as *const libc::c_void, prompt.len());
        }
    }

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
            pfds.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }
        if let Some(fd) = socket_fd {
            pfds.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }

        let ret = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout_ms) };
        if ret <= 0 {
            continue;
        }

        for pfd in &pfds {
            if pfd.revents & libc::POLLIN != 0 {
                if Some(pfd.fd) == tty_fd {
                    let mut ch: libc::c_char = 0;
                    let n = unsafe {
                        libc::read(pfd.fd, &mut ch as *mut libc::c_char as *mut libc::c_void, 1)
                    };
                    if n == 1 {
                        match ch as u8 {
                            b'y' | b'Y' | b'1' => return Decision::Allow,
                            b'n' | b'N' | b'2' | 3 | 27 => return Decision::Deny,
                            _ => {}
                        }
                    }
                } else if Some(pfd.fd) == socket_fd {
                    let mut byte = 0u8;
                    let n =
                        unsafe { libc::read(pfd.fd, &mut byte as *mut u8 as *mut libc::c_void, 1) };
                    if n <= 0 {
                        return Decision::Fallback;
                    }
                    if byte == b'\n' {
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
            if pfd.revents & (libc::POLLHUP | libc::POLLERR) != 0 && Some(pfd.fd) == socket_fd {
                return Decision::Fallback;
            }
        }
    }
}

// ── Windows-only: console keypress + socket directive dual-channel wait ───────

#[cfg(windows)]
mod win_console {
    use std::sync::mpsc::SyncSender;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Console::{
        GetConsoleMode, ReadConsoleInputW, SetConsoleMode, INPUT_RECORD, KEY_EVENT,
    };

    use windows::Win32::System::Console::CONSOLE_MODE;

    pub struct ConsoleModeGuard {
        handle: HANDLE,
        saved: CONSOLE_MODE,
    }

    impl ConsoleModeGuard {
        pub fn new(handle: HANDLE) -> Option<Self> {
            let mut mode = CONSOLE_MODE(0);
            if unsafe { GetConsoleMode(handle, &mut mode) }.is_ok() {
                Some(Self {
                    handle,
                    saved: mode,
                })
            } else {
                None
            }
        }

        pub fn saved(&self) -> CONSOLE_MODE {
            self.saved
        }
    }

    impl Drop for ConsoleModeGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = SetConsoleMode(self.handle, self.saved);
            }
        }
    }

    pub fn open_console_handle(name: &str, write: bool) -> Option<HANDLE> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let access = if write { GENERIC_WRITE } else { GENERIC_READ };
        unsafe {
            CreateFileW(
                windows::core::PCWSTR(wide.as_ptr()),
                access.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                Default::default(),
                None,
            )
            .ok()
        }
    }

    pub fn spawn_keypress_reader(conin: HANDLE, tx: SyncSender<super::Decision>) {
        let handle_val = conin.0 as isize;
        std::thread::spawn(move || {
            let conin = HANDLE(handle_val as *mut core::ffi::c_void);
            let mut buf = [INPUT_RECORD::default(); 1];
            let mut count = 0u32;
            loop {
                if unsafe { ReadConsoleInputW(conin, &mut buf, &mut count) }.is_err() {
                    break;
                }
                if count == 0 {
                    continue;
                }
                let rec = &buf[0];
                if rec.EventType == KEY_EVENT as u16 {
                    let ke = unsafe { &rec.Event.KeyEvent };
                    if ke.bKeyDown.as_bool() {
                        let ch = unsafe { ke.uChar.UnicodeChar };
                        let decision = match ch {
                            89 | 121 | 49 => {  // 'Y', 'y', '1'
                                Some(super::Decision::Allow)
                            }
                            78 | 110 | 50 => {  // 'N', 'n', '2'
                                Some(super::Decision::Deny)
                            }
                            27 /* Esc */ | 3 /* Ctrl-C */ => {
                                Some(super::Decision::Deny)
                            }
                            _ => None,
                        };
                        if let Some(d) = decision {
                            let _ = tx.send(d);
                            break;
                        }
                    }
                }
            }
            unsafe {
                let _ = CloseHandle(conin);
            }
        });
    }
}

#[cfg(windows)]
fn wait_for_decision_windows(
    socket: &mut Option<BufReader<TcpStream>>,
    tool_name: &str,
    tool_input: &serde_json::Value,
) -> Decision {
    use windows::Win32::System::Console::{
        SetConsoleMode, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
    };

    // Print the Open Island prompt to CONOUT$.
    if let Some(conout) = win_console::open_console_handle("CONOUT$", true) {
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
        let wide: Vec<u16> = prompt.encode_utf16().collect();
        let mut written = 0u32;
        unsafe {
            use windows::Win32::System::Console::WriteConsoleW;
            let _ = WriteConsoleW(conout, &wide, Some(&mut written), None);
            use windows::Win32::Foundation::CloseHandle;
            let _ = CloseHandle(conout);
        }
    }

    // Open CONIN$ in raw mode.
    let conin = match win_console::open_console_handle("CONIN$", false) {
        Some(h) => h,
        None => return Decision::Fallback,
    };

    let _mode_guard = win_console::ConsoleModeGuard::new(conin);
    if let Some(ref guard) = _mode_guard {
        unsafe {
            let saved = guard.saved().0;
            let raw_mode =
                saved & !(ENABLE_LINE_INPUT.0 | ENABLE_ECHO_INPUT.0 | ENABLE_PROCESSED_INPUT.0);
            let _ = SetConsoleMode(
                conin,
                windows::Win32::System::Console::CONSOLE_MODE(raw_mode),
            );
        }
    }

    let (tx, rx) = std::sync::mpsc::sync_channel::<Decision>(1);
    let tx_key = tx.clone();

    // Spawn keypress reader thread.
    win_console::spawn_keypress_reader(conin, tx_key);

    // Main thread: read further lines from socket looking for a directive.
    if let Some(reader) = socket.take() {
        let tx_sock = tx.clone();
        std::thread::spawn(move || {
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        eprintln!("[oi-hook] socket line: {l}");
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&l) {
                            let directive_type = val["response"]["directive"]["type"].as_str();
                            eprintln!("[oi-hook] parsed directive_type={directive_type:?}");
                            match directive_type {
                                Some("allow") => {
                                    let _ = tx_sock.send(Decision::Allow);
                                    eprintln!("[oi-hook] sent Decision::Allow");
                                    break;
                                }
                                Some("deny") => {
                                    let _ = tx_sock.send(Decision::Deny);
                                    eprintln!("[oi-hook] sent Decision::Deny");
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[oi-hook] socket read err: {e}");
                        break;
                    }
                }
            }
            eprintln!("[oi-hook] socket reader thread exiting");
        });
    }

    match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(d) => d,
        Err(_) => Decision::Fallback,
    }
}

// ── Windows-only: inject terminal identification into the hook payload ────────

#[cfg(windows)]
mod win_terminal {
    use std::collections::HashMap;
    use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER,
    };

    // Only graphical terminal hosts that own a window — NOT shells like powershell/cmd
    // which are child processes and don't have their own top-level window.
    const TERMINAL_APPS: &[&str] = &[
        "WindowsTerminal.exe",
        "OpenConsole.exe",
        "conhost.exe",
        "alacritty.exe",
        "wezterm-gui.exe",
        "mintty.exe",
        "Hyper.exe",
        "Tabby.exe",
    ];

    fn process_map() -> HashMap<u32, (u32, String)> {
        let mut map = HashMap::new();
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        let snap = match snap {
            Ok(h) => h,
            Err(_) => return map,
        };
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if unsafe { Process32FirstW(snap, &mut entry) }.is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                map.insert(entry.th32ProcessID, (entry.th32ParentProcessID, name));
                if unsafe { Process32NextW(snap, &mut entry) }.is_err() {
                    break;
                }
            }
        }
        unsafe {
            let _ = CloseHandle(snap);
        }
        map
    }

    struct HwndSearch {
        pid: u32,
        hwnd: isize,
    }

    unsafe extern "system" fn hwnd_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let s = unsafe { &mut *(lparam.0 as *mut HwndSearch) };
        let mut wnd_pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut wnd_pid));
        }
        if wnd_pid == s.pid && unsafe { IsWindowVisible(hwnd).as_bool() } {
            let is_top_level = match unsafe { GetWindow(hwnd, GW_OWNER) } {
                Ok(owner) => owner.0.is_null(),
                Err(_) => true,
            };
            if is_top_level {
                s.hwnd = hwnd.0 as isize;
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    fn hwnd_for_pid(pid: u32) -> Option<isize> {
        let mut search = HwndSearch { pid, hwnd: 0 };
        unsafe {
            let _ = EnumWindows(
                Some(hwnd_callback),
                LPARAM(&mut search as *mut HwndSearch as isize),
            );
        }
        if search.hwnd != 0 {
            Some(search.hwnd)
        } else {
            None
        }
    }

    /// Walk the parent process tree to find the nearest terminal host.
    /// Returns (hwnd, terminal_app_name, terminal_pid_str, wt_session_guid).
    pub fn terminal_info() -> (
        Option<isize>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        let map = process_map();
        let my_pid = unsafe { GetCurrentProcessId() };

        let claude_pid = map.get(&my_pid).map(|(ppid, _)| *ppid);
        let shell_pid = claude_pid.and_then(|p| map.get(&p).map(|(ppid, _)| *ppid));

        let mut terminal_pid: Option<u32> = None;
        let mut terminal_app: Option<String> = None;

        if let Some(cpid) = claude_pid {
            let mut cur = cpid;
            for _ in 0..12 {
                match map.get(&cur) {
                    Some((ppid, name)) => {
                        if TERMINAL_APPS.iter().any(|t| name.eq_ignore_ascii_case(t)) {
                            terminal_pid = Some(cur);
                            terminal_app = Some(name.clone());
                            break;
                        }
                        let next = *ppid;
                        if next == 0 || next == cur {
                            break;
                        }
                        cur = next;
                    }
                    None => break,
                }
            }
        }

        // Fallback for standalone conhost (non-WT): conhost.exe is a child of the shell,
        // not a parent, so the upward walk misses it.
        if terminal_pid.is_none() {
            if let Some(spid) = shell_pid {
                for (pid, (ppid, name)) in &map {
                    if *ppid == spid && name.eq_ignore_ascii_case("conhost.exe") {
                        terminal_pid = Some(*pid);
                        terminal_app = Some(name.clone());
                        break;
                    }
                }
            }
        }

        let hwnd = terminal_pid.and_then(hwnd_for_pid);
        let wt_session = std::env::var("WT_SESSION").ok();
        eprintln!(
            "[oi-hook] terminal_info: claude_pid={:?} shell_pid={:?} terminal_pid={:?} app={:?} hwnd={:?}",
            claude_pid, shell_pid, terminal_pid, terminal_app, hwnd
        );
        (
            hwnd,
            terminal_app,
            terminal_pid.map(|p| p.to_string()),
            wt_session,
        )
    }

    /// Set the Windows Terminal tab title via CONOUT$ so UIA can match it later.
    pub fn set_tab_title(title: &str) {
        use std::io::Write;
        let seq = format!("\x1b]0;{}\x07", title);
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
            let _ = f.write_all(seq.as_bytes());
        }
    }
}

fn main() {
    let event_arg = std::env::args().nth(1).unwrap_or_default();
    eprintln!("[open-island-hook] invoked event={event_arg}");
    let log_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut payload = String::new();
    if io::stdin().read_to_string(&mut payload).is_err() {
        eprintln!("[open-island-hook] failed to read stdin");
        return;
    }
    let payload = payload.trim().to_string();
    if payload.is_empty() {
        eprintln!("[open-island-hook] empty payload, exiting");
        return;
    }

    let mut parsed: serde_json::Value = match serde_json::from_str(&payload) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[open-island-hook] JSON parse error: {e}");
            return;
        }
    };

    let event = parsed["hook_event_name"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    let tool_name = parsed["tool_name"].as_str().unwrap_or("").to_string();
    let needs_permission =
        (event == "pretooluse" || event == "permissionrequest") && requires_approval(&tool_name);

    // Windows: inject terminal identification fields and set WT tab title.
    #[cfg(windows)]
    {
        let (hwnd, term_app, term_pid, wt_session) = win_terminal::terminal_info();
        let hwnd_log = hwnd
            .map(|h| h.to_string())
            .unwrap_or_else(|| "none".to_string());
        let app_log = term_app.clone().unwrap_or_else(|| "none".to_string());
        {
            let log_path = std::env::temp_dir().join("oi-hook-log.txt");
            let line = format!("ts={log_ts} event={event_arg} hwnd={hwnd_log} app={app_log}\n");
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(line.as_bytes())
                });
        }
        if let Some(obj) = parsed.as_object_mut() {
            if let Some(v) = hwnd {
                obj.entry("terminal_window_id")
                    .or_insert_with(|| serde_json::Value::String(v.to_string()));
            }
            if let Some(v) = term_app.clone() {
                obj.entry("terminal_app")
                    .or_insert_with(|| serde_json::Value::String(v));
            }
            if let Some(v) = term_pid {
                obj.entry("terminal_pid")
                    .or_insert_with(|| serde_json::Value::String(v));
            }
            if let Some(v) = wt_session {
                obj.entry("terminal_session_id")
                    .or_insert_with(|| serde_json::Value::String(v));
            }
        }
        if event == "sessionstart" && term_app.as_deref() == Some("WindowsTerminal.exe") {
            let sid = parsed["session_id"].as_str().unwrap_or("");
            if !sid.is_empty() {
                let token = format!("OI-{}", &sid[..sid.len().min(8)]);
                win_terminal::set_tab_title(&token);
            }
        }
    }
    #[cfg(not(windows))]
    {
        let log_path = std::env::temp_dir().join("oi-hook-log.txt");
        let line = format!("ts={log_ts} event={event_arg}\n");
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(line.as_bytes())
            });
    }

    let envelope = serde_json::json!({
        "type": "command",
        "command": {
            "type": "processClaudeHook",
            "claudeHook": parsed
        }
    });

    // Forward to bridge over TCP. For gated tools, keep the socket open and wait
    // for a ClaudeHookDirective pushed back by the bridge (pill Allow/Deny), or
    // for a console keypress — whichever comes first.
    let mut socket_opt: Option<BufReader<TcpStream>> = None;

    match read_port() {
        None => eprintln!("[open-island-hook] no port file found"),
        Some(port) => {
            eprintln!("[open-island-hook] connecting to 127.0.0.1:{port}");
            match TcpStream::connect(("127.0.0.1", port)) {
                Err(e) => eprintln!("[open-island-hook] connect failed: {e}"),
                Ok(mut s) => {
                    eprintln!("[open-island-hook] connected, sending envelope");
                    if let Ok(msg) = serde_json::to_string(&envelope) {
                        s.set_write_timeout(Some(Duration::from_secs(3))).ok();
                        // No read timeout — for gated tools we may wait up to 30 s.
                        if s.write_all(msg.as_bytes()).is_ok() && s.write_all(b"\n").is_ok() {
                            eprintln!("[open-island-hook] sent, waiting for ack");
                            // Read the ack line.
                            let mut reader = BufReader::new(s);
                            let mut ack_line = String::new();
                            match reader.read_line(&mut ack_line) {
                                Ok(0) | Err(_) => eprintln!("[open-island-hook] ack read failed"),
                                Ok(_) => {
                                    eprintln!("[open-island-hook] ack received");
                                    if needs_permission {
                                        // Keep BufReader for the directive read loop.
                                        socket_opt = Some(reader);
                                    }
                                }
                            }
                        } else {
                            eprintln!("[open-island-hook] write failed");
                        }
                    }
                }
            }
        }
    }

    if needs_permission {
        let decision = {
            #[cfg(windows)]
            {
                wait_for_decision_windows(&mut socket_opt, &tool_name, &parsed["tool_input"])
            }
            #[cfg(unix)]
            {
                use std::os::unix::io::AsRawFd;
                let fd = socket_opt.as_ref().map(|s| s.get_ref().as_raw_fd());
                wait_for_decision(fd, &tool_name, &parsed["tool_input"])
            }
        };
        drop(socket_opt);

        let permission_decision = match decision {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Fallback => "ask",
        };
        let out = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": permission_decision
            }
        });
        let _ = io::stdout().write_all(serde_json::to_string(&out).unwrap().as_bytes());
    }
}
