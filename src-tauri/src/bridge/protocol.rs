use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BridgeEnvelope {
    Hello(BridgeHello),
    #[serde(rename = "event")]
    Event { event: AgentEvent },
    #[serde(rename = "command")]
    Command { command: BridgeCommand },
    #[serde(rename = "response")]
    Response { response: BridgeResponse },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeHello {
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ClientRole {
    Hook,
    Ui,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BridgeCommand {
    RegisterClient {
        role: ClientRole,
    },
    ProcessClaudeHook {
        #[serde(rename = "claudeHook")]
        claude_hook: ClaudeHookPayload,
    },
    ResolvePermission {
        #[serde(rename = "sessionID")]
        session_id: String,
        resolution: PermissionResolution,
    },
    AnswerQuestion {
        #[serde(rename = "sessionID")]
        session_id: String,
        response: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BridgeResponse {
    Acknowledged,
    ClaudeHookDirective {
        directive: ClaudeHookDirective,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeHookPayload {
    pub session_id: String,
    pub hook_event_name: String,
    pub cwd: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub tool_use_id: Option<String>,
    pub tool_response: Option<Value>,
    pub permission_mode: Option<String>,
    pub permission_suggestions: Option<Vec<Value>>,
    pub last_assistant_message: Option<String>,
    pub message: Option<String>,
    pub title: Option<String>,
    pub notification_type: Option<String>,
    pub transcript_path: Option<String>,
    pub terminal_app: Option<String>,
    pub terminal_tty: Option<String>,
    pub terminal_session_id: Option<String>,
    pub terminal_title: Option<String>,
    pub model: Option<String>,
    pub is_interrupt: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClaudeHookDirective {
    Allow,
    Deny { message: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PermissionResolution {
    Allow,
    Deny { message: Option<String> },
}

// ── Agent events emitted to UI clients ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentEvent {
    SessionStarted(SessionStarted),
    ActivityUpdated(SessionActivityUpdated),
    PermissionRequested(PermissionRequested),
    SessionCompleted(SessionCompleted),
    ActionableStateResolved(ActionableStateResolved),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStarted {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub timestamp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionActivityUpdated {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub summary: Option<String>,
    pub phase: Option<String>,
    pub timestamp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequested {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub tool_name: String,
    pub tool_input: Option<Value>,
    pub timestamp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCompleted {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub summary: Option<String>,
    pub timestamp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionableStateResolved {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub timestamp: f64,
}
