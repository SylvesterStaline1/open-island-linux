use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;

use super::protocol::*;
use super::state::{AgentSession, BridgeState, PendingPermission, SessionPhase};

pub type SessionsSnapshot = Vec<AgentSession>;

#[derive(Debug)]
pub enum ServerEvent {
    SessionsChanged(SessionsSnapshot),
    PermissionRequested {
        session_id: String,
        tool_name: String,
        tool_input: Option<Value>,
    },
    PermissionResolved(String),
    Notification {
        title: Option<String>,
        message: String,
    },
}

struct PendingApproval {
    tx: oneshot::Sender<ClaudeHookDirective>,
}

pub(crate) struct ServerInner {
    state: BridgeState,
    pending_approvals: HashMap<String, PendingApproval>,
    event_tx: mpsc::UnboundedSender<ServerEvent>,
}

impl ServerInner {
    fn now() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
    }

    fn emit_snapshot(&self) {
        let _ = self.event_tx.send(ServerEvent::SessionsChanged(
            self.state.sessions_snapshot(),
        ));
    }

    fn handle_claude_hook(&mut self, payload: ClaudeHookPayload) -> Option<oneshot::Receiver<ClaudeHookDirective>> {
        let ts = Self::now();
        let sid = payload.session_id.clone();

        let event = payload.hook_event_name.to_lowercase();
        match event.as_str() {
            "sessionstart" => {
                self.state.get_or_create(&sid, payload.cwd.clone(), None, ts);
                self.emit_snapshot();
                None
            }

            "pretooluse" => {
                let tool_name = payload.tool_name.clone().unwrap_or_default();
                let tool_input = payload.tool_input.clone();

                let needs_approval = requires_approval(&tool_name);

                let session = self.state.get_or_create(&sid, payload.cwd.clone(), None, ts);
                session.updated_at = ts;

                if needs_approval {
                    session.phase = SessionPhase::AwaitingPermission;
                    session.pending_permission = Some(PendingPermission {
                        tool_name: tool_name.clone(),
                        tool_input: tool_input.clone(),
                    });
                    self.emit_snapshot();

                    let _ = self.event_tx.send(ServerEvent::PermissionRequested {
                        session_id: sid.clone(),
                        tool_name,
                        tool_input,
                    });

                    let (tx, rx) = oneshot::channel();
                    self.pending_approvals.insert(sid, PendingApproval { tx });
                    Some(rx)
                } else {
                    session.phase = SessionPhase::Working;
                    session.summary = Some(format!("Running {}", tool_name));
                    self.emit_snapshot();
                    None
                }
            }

            "permissionrequest" => {
                let tool_name = payload.tool_name.clone().unwrap_or_default();
                let tool_input = payload.tool_input.clone();

                let session = self.state.get_or_create(&sid, payload.cwd.clone(), None, ts);
                session.phase = SessionPhase::AwaitingPermission;
                session.updated_at = ts;
                session.pending_permission = Some(PendingPermission {
                    tool_name: tool_name.clone(),
                    tool_input: tool_input.clone(),
                });
                self.emit_snapshot();

                let _ = self.event_tx.send(ServerEvent::PermissionRequested {
                    session_id: sid.clone(),
                    tool_name,
                    tool_input,
                });

                let (tx, rx) = oneshot::channel();
                self.pending_approvals.insert(sid, PendingApproval { tx });
                Some(rx)
            }

            "posttooluse" | "posttoolusefailure" => {
                if let Some(session) = self.state.sessions.get_mut(&sid) {
                    session.phase = SessionPhase::Working;
                    session.pending_permission = None;
                    session.updated_at = ts;
                }
                self.emit_snapshot();
                None
            }

            "stop" | "sessionend" => {
                if let Some(session) = self.state.sessions.get_mut(&sid) {
                    session.phase = SessionPhase::Completed;
                    session.updated_at = ts;
                }
                self.emit_snapshot();
                None
            }

            "notification" | "userpromptsubmit" => {
                let _ = self.event_tx.send(ServerEvent::Notification {
                    title: payload.title.clone(),
                    message: payload.message.clone().unwrap_or_default(),
                });
                None
            }

            _ => {
                // For any other hook event, update the session as active
                if let Some(session) = self.state.sessions.get_mut(&sid) {
                    session.updated_at = ts;
                    if let Some(msg) = &payload.last_assistant_message {
                        if !msg.is_empty() {
                            let truncated = msg.chars().take(80).collect::<String>();
                            session.summary = Some(truncated);
                        }
                    }
                }
                self.emit_snapshot();
                None
            }
        }
    }

    fn resolve_permission(&mut self, session_id: &str, resolution: PermissionResolution) {
        if let Some(approval) = self.pending_approvals.remove(session_id) {
            let directive = match resolution {
                PermissionResolution::Allow => ClaudeHookDirective::Allow,
                PermissionResolution::Deny { message } => ClaudeHookDirective::Deny { message },
            };
            let _ = approval.tx.send(directive);
        }

        if let Some(session) = self.state.sessions.get_mut(session_id) {
            session.phase = SessionPhase::Working;
            session.pending_permission = None;
        }

        let _ = self.event_tx.send(ServerEvent::PermissionResolved(session_id.to_string()));
        self.emit_snapshot();
    }
}

pub struct BridgeServer {
    inner: Arc<Mutex<ServerInner>>,
    pub event_rx: Option<mpsc::UnboundedReceiver<ServerEvent>>,
}

impl BridgeServer {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            inner: Arc::new(Mutex::new(ServerInner {
                state: BridgeState::default(),
                pending_approvals: HashMap::new(),
                event_tx: tx,
            })),
            event_rx: Some(rx),
        }
    }

    pub fn socket_path() -> PathBuf {
        // Honour env override (compatible with open-vibe-island hooks)
        if let Ok(p) = std::env::var("OPEN_ISLAND_SOCKET_PATH") {
            return PathBuf::from(p);
        }
        if let Ok(p) = std::env::var("VIBE_ISLAND_SOCKET_PATH") {
            return PathBuf::from(p);
        }
        let base = dirs::runtime_dir()
            .or_else(|| dirs::data_local_dir())
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        base.join("open-island").join("bridge.sock")
    }

    pub(crate) async fn listen(inner: Arc<Mutex<ServerInner>>, socket_path: PathBuf) -> Result<()> {
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        log::info!("Bridge server listening on {:?}", socket_path);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let inner = inner.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(stream, inner).await {
                            log::debug!("Client disconnected: {}", e);
                        }
                    });
                }
                Err(e) => log::error!("Accept error: {}", e),
            }
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        let path = Self::socket_path();
        let inner = self.inner.clone();
        tokio::spawn(Self::listen(inner, path));
        Ok(())
    }

    pub async fn resolve_permission(&self, session_id: String, resolution: PermissionResolution) {
        let mut inner = self.inner.lock().await;
        inner.resolve_permission(&session_id, resolution);
    }

    pub async fn sessions_snapshot(&self) -> SessionsSnapshot {
        let inner = self.inner.lock().await;
        inner.state.sessions_snapshot()
    }
}

async fn handle_client(stream: UnixStream, inner: Arc<Mutex<ServerInner>>) -> Result<()> {
    let client_id = Uuid::new_v4().to_string();
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut lines = BufReader::new(read_half).lines();

    // Channels for deferred hook responses
    let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<String>();

    let write_task = tokio::spawn(async move {
        while let Some(line) = resp_rx.recv().await {
            let _ = write_half.write_all(line.as_bytes()).await;
            let _ = write_half.write_all(b"\n").await;
        }
    });

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let envelope: BridgeEnvelope = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(e) => {
                log::warn!("[{}] Failed to parse envelope: {} — raw: {}", client_id, e, &line[..line.len().min(120)]);
                continue;
            }
        };

        match envelope {
            BridgeEnvelope::Hello(_) => {
                let ack = serde_json::to_string(&BridgeEnvelope::Response {
                    response: BridgeResponse::Acknowledged,
                })?;
                resp_tx.send(ack)?;
            }

            BridgeEnvelope::Command { command } => {
                match command {
                    BridgeCommand::RegisterClient { .. } => {
                        let ack = serde_json::to_string(&BridgeEnvelope::Response {
                            response: BridgeResponse::Acknowledged,
                        })?;
                        resp_tx.send(ack)?;
                    }

                    BridgeCommand::ProcessClaudeHook { claude_hook } => {
                        let approval_rx = {
                            let mut guard = inner.lock().await;
                            guard.handle_claude_hook(claude_hook)
                        };

                        if let Some(rx) = approval_rx {
                            let resp_tx = resp_tx.clone();
                            tokio::spawn(async move {
                                let directive = rx.await.unwrap_or(ClaudeHookDirective::Allow);
                                let env = BridgeEnvelope::Response {
                                    response: BridgeResponse::ClaudeHookDirective { directive },
                                };
                                if let Ok(json) = serde_json::to_string(&env) {
                                    let _ = resp_tx.send(json);
                                }
                            });
                        } else {
                            let ack = serde_json::to_string(&BridgeEnvelope::Response {
                                response: BridgeResponse::Acknowledged,
                            })?;
                            resp_tx.send(ack)?;
                        }
                    }

                    BridgeCommand::ResolvePermission { session_id, resolution } => {
                        let mut guard = inner.lock().await;
                        guard.resolve_permission(&session_id, resolution);
                        let ack = serde_json::to_string(&BridgeEnvelope::Response {
                            response: BridgeResponse::Acknowledged,
                        })?;
                        resp_tx.send(ack)?;
                    }

                    BridgeCommand::AnswerQuestion { .. } => {
                        let ack = serde_json::to_string(&BridgeEnvelope::Response {
                            response: BridgeResponse::Acknowledged,
                        })?;
                        resp_tx.send(ack)?;
                    }
                }
            }

            _ => {}
        }
    }

    drop(resp_tx);
    let _ = write_task.await;
    Ok(())
}

// Tools that can modify filesystem, run shell commands, or have side effects
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
