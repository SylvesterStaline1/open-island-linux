use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SessionPhase {
    Idle,
    Working,
    AwaitingPermission,
    AwaitingQuestion,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPermission {
    pub tool_name: String,
    pub tool_input: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub session_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub phase: SessionPhase,
    pub summary: Option<String>,
    pub pending_permission: Option<PendingPermission>,
    pub pending_question: Option<String>,
    pub terminal_tty: Option<String>,
    pub terminal_window_id: Option<String>,
    pub terminal_app: Option<String>,
    pub terminal_session_id: Option<String>,
    pub terminal_pid: Option<String>,
    pub started_at: f64,
    pub updated_at: f64,
}

impl AgentSession {
    pub fn new(session_id: String, cwd: Option<String>, title: Option<String>, timestamp: f64) -> Self {
        Self {
            session_id,
            title,
            cwd,
            phase: SessionPhase::Working,
            summary: None,
            pending_permission: None,
            pending_question: None,
            terminal_tty: None,
            terminal_window_id: None,
            terminal_app: None,
            terminal_session_id: None,
            terminal_pid: None,
            started_at: timestamp,
            updated_at: timestamp,
        }
    }
}

#[derive(Debug, Default)]
pub struct BridgeState {
    pub sessions: HashMap<String, AgentSession>,
}

impl BridgeState {
    pub fn get_or_create(&mut self, session_id: &str, cwd: Option<String>, title: Option<String>, timestamp: f64) -> &mut AgentSession {
        self.sessions
            .entry(session_id.to_string())
            .or_insert_with(|| AgentSession::new(session_id.to_string(), cwd, title, timestamp))
    }

    pub fn sessions_snapshot(&self) -> Vec<AgentSession> {
        self.sessions.values().cloned().collect()
    }
}
