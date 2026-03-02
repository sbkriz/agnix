//! Kiro custom agent JSON schema helpers.
//!
//! Covers `.kiro/agents/*.json` payloads and embedded CLI hook structures.
#![allow(dead_code)]

use crate::schemas::kiro_mcp::KiroMcpServerConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Supported model values documented by Kiro.
pub const VALID_KIRO_AGENT_MODELS: &[&str] = &[
    "claude-sonnet-4",
    "claude-sonnet4.5",
    "claude-sonnet-4-5",
    "claude-opus4.5",
    "claude-opus-4-5",
    "Auto",
];

/// JSON parse error with location metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

/// Parsed Kiro agent document.
#[derive(Debug, Clone)]
pub struct ParsedKiroAgentConfig {
    pub config: Option<KiroAgentConfig>,
    pub parse_error: Option<ParseError>,
    pub raw_value: Option<Value>,
}

/// Kiro custom agent JSON schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroAgentConfig {
    pub name: Option<String>,
    pub description: Option<String>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub tools: Option<Vec<String>>,
    pub allowed_tools: Option<Vec<String>>,
    pub tool_aliases: Option<HashMap<String, String>>,
    pub tools_settings: Option<Value>,
    pub resources: Option<Vec<KiroAgentResource>>,
    pub mcp_servers: Option<KiroMcpServers>,
    pub include_mcp_json: Option<bool>,
    pub hooks: Option<KiroCliHooks>,
    pub keyboard_shortcut: Option<String>,
    pub welcome_message: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Resource entries allowed by Kiro agent config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KiroAgentResource {
    Uri(String),
    Structured(Value),
}

/// `mcpServers` may be inline server map or named server list, depending on source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KiroMcpServers {
    Names(Vec<String>),
    Servers(HashMap<String, KiroMcpServerConfig>),
}

/// CLI hook mapping in Kiro custom agents.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroCliHooks {
    pub agent_spawn: Option<Vec<KiroCliHookEntry>>,
    pub user_prompt_submit: Option<Vec<KiroCliHookEntry>>,
    pub pre_tool_use: Option<Vec<KiroCliHookEntry>>,
    pub post_tool_use: Option<Vec<KiroCliHookEntry>>,
    pub stop: Option<Vec<KiroCliHookEntry>>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Individual CLI hook entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroCliHookEntry {
    pub command: Option<String>,
    pub matcher: Option<String>,
    pub timeout_ms: Option<u64>,
    pub cache_ttl_seconds: Option<u64>,
    pub tool_types: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Parse Kiro custom-agent JSON into typed schema and raw value.
pub fn parse_kiro_agent_config(content: &str) -> ParsedKiroAgentConfig {
    let raw_value = serde_json::from_str::<Value>(content).ok();
    match serde_json::from_str::<KiroAgentConfig>(content) {
        Ok(config) => ParsedKiroAgentConfig {
            config: Some(config),
            parse_error: None,
            raw_value,
        },
        Err(err) => ParsedKiroAgentConfig {
            config: None,
            parse_error: Some(ParseError {
                message: err.to_string(),
                line: err.line(),
                column: err.column(),
            }),
            raw_value,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_agent_config_with_named_mcp_servers() {
        let parsed = parse_kiro_agent_config(
            r#"{
  "name": "review-agent",
  "model": "claude-sonnet-4",
  "tools": ["readFiles"],
  "allowedTools": ["readFiles"],
  "resources": ["file://docs/README.md", {"type":"knowledgeBase","id":"kb"}],
  "mcpServers": {
    "filesystem": {
      "command": "node",
      "args": ["server.js"]
    }
  },
  "hooks": {
    "preToolUse": [{"command":"echo pre"}]
  }
}"#,
        );

        assert!(parsed.parse_error.is_none());
        let config = parsed.config.expect("config should parse");
        assert_eq!(config.name.as_deref(), Some("review-agent"));
        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4"));
        assert!(matches!(
            config.mcp_servers,
            Some(KiroMcpServers::Servers(_))
        ));
    }

    #[test]
    fn parse_valid_agent_config_with_mcp_server_name_list() {
        let parsed = parse_kiro_agent_config(
            r#"{
  "name": "review-agent",
  "mcpServers": ["filesystem", "github"]
}"#,
        );

        assert!(parsed.parse_error.is_none());
        let config = parsed.config.expect("config should parse");
        assert!(matches!(config.mcp_servers, Some(KiroMcpServers::Names(_))));
    }

    #[test]
    fn parse_invalid_agent_config_reports_error_location() {
        let parsed = parse_kiro_agent_config(r#"{"name":"broken","tools":[}"#);
        let error = parsed.parse_error.expect("expected parse error");
        assert!(error.line > 0);
        assert!(error.column > 0);
    }
}
