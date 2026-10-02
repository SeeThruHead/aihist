use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Tool {
    Claude,
    Codex,
    OpenCode,
}

impl Tool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::OpenCode => "opencode",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Tool::Claude),
            "codex" => Some(Tool::Codex),
            "opencode" => Some(Tool::OpenCode),
            _ => None,
        }
    }
}

impl fmt::Display for Tool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    ToolUse,
    ToolResult,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::ToolUse => "tool_use",
            Role::ToolResult => "tool_result",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Role::User),
            "assistant" => Some(Role::Assistant),
            "tool_use" => Some(Role::ToolUse),
            "tool_result" => Some(Role::ToolResult),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub tool: Tool,
    pub title: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub tokens_input: i64,
    pub tokens_output: i64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Turn {
    pub id: i64,
    pub session_id: String,
    pub seq: i32,
    pub role: Role,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
    pub content: String,
    pub ts: Option<i64>,
    pub tokens: i64,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub session_id: String,
    pub tool: Tool,
    pub seq: i32,
    pub role: Role,
    pub tool_name: Option<String>,
    pub snippet: String,
    pub rank: f64,
    pub started_at: i64,
    pub project: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub tool: Option<Tool>,
    pub since_ms: Option<i64>,
    pub limit: Option<usize>,
    pub role: Option<Role>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_roundtrip() {
        for tool in [Tool::Claude, Tool::Codex, Tool::OpenCode] {
            assert_eq!(Tool::from_str(tool.as_str()), Some(tool));
        }
    }

    #[test]
    fn tool_from_unknown_str_is_none() {
        assert_eq!(Tool::from_str("unknown"), None);
    }

    #[test]
    fn role_roundtrip() {
        for role in [Role::User, Role::Assistant, Role::ToolUse, Role::ToolResult] {
            assert_eq!(Role::from_str(role.as_str()), Some(role));
        }
    }

    #[test]
    fn tool_display() {
        assert_eq!(Tool::Claude.to_string(), "claude");
        assert_eq!(Tool::Codex.to_string(), "codex");
        assert_eq!(Tool::OpenCode.to_string(), "opencode");
    }
}
