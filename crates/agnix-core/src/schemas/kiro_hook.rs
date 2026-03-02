//! Kiro IDE hook file schema helpers.
//!
//! Covers `.kiro/hooks/*.kiro.hook` JSON payloads.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Valid Kiro IDE hook event names.
pub const VALID_KIRO_HOOK_EVENTS: &[&str] = &[
    "fileEdited",
    "fileCreate",
    "fileDelete",
    "promptSubmit",
    "agentStop",
    "preToolUse",
    "postToolUse",
    "manual",
];

/// JSON parse error with line/column metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

/// Parsed Kiro hook document.
#[derive(Debug, Clone)]
pub struct ParsedKiroHook {
    pub hook: Option<KiroIdeHook>,
    pub parse_error: Option<ParseError>,
    pub raw_value: Option<Value>,
}

/// Kiro IDE hook structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroIdeHook {
    pub title: Option<String>,
    pub description: Option<String>,
    pub event: Option<String>,
    pub patterns: Option<Vec<String>>,
    pub tool_types: Option<Vec<String>>,
    pub then: Option<KiroHookAction>,
    pub run_command: Option<String>,
    pub ask_agent: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Optional nested action block used by some hook representations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroHookAction {
    pub run_command: Option<String>,
    pub ask_agent: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl KiroIdeHook {
    /// Resolve an effective command from top-level or nested `then`.
    #[allow(dead_code)]
    pub fn effective_run_command(&self) -> Option<&str> {
        self.run_command
            .as_deref()
            .or_else(|| self.then.as_ref()?.run_command.as_deref())
    }

    /// Resolve an effective `askAgent` target from top-level or nested `then`.
    #[allow(dead_code)]
    pub fn effective_ask_agent(&self) -> Option<&str> {
        self.ask_agent
            .as_deref()
            .or_else(|| self.then.as_ref()?.ask_agent.as_deref())
    }

    /// True if either supported action is configured.
    #[allow(dead_code)]
    pub fn has_action(&self) -> bool {
        self.effective_run_command().is_some() || self.effective_ask_agent().is_some()
    }
}

/// Parse Kiro IDE hook JSON into typed schema and raw value.
pub fn parse_kiro_hook(content: &str) -> ParsedKiroHook {
    let raw_value = serde_json::from_str::<Value>(content).ok();
    match serde_json::from_str::<KiroIdeHook>(content) {
        Ok(hook) => ParsedKiroHook {
            hook: Some(hook),
            parse_error: None,
            raw_value,
        },
        Err(err) => ParsedKiroHook {
            hook: None,
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
    fn parse_valid_hook_top_level_actions() {
        let parsed = parse_kiro_hook(
            r#"{
  "event": "fileEdited",
  "patterns": ["**/*.md"],
  "runCommand": "echo changed"
}"#,
        );

        assert!(parsed.parse_error.is_none());
        let hook = parsed.hook.expect("hook should parse");
        assert_eq!(hook.event.as_deref(), Some("fileEdited"));
        assert_eq!(hook.effective_run_command(), Some("echo changed"));
        assert!(hook.has_action());
    }

    #[test]
    fn parse_valid_hook_nested_then_actions() {
        let parsed = parse_kiro_hook(
            r#"{
  "event": "promptSubmit",
  "then": {
    "askAgent": "review-agent"
  }
}"#,
        );

        assert!(parsed.parse_error.is_none());
        let hook = parsed.hook.expect("hook should parse");
        assert_eq!(hook.effective_ask_agent(), Some("review-agent"));
        assert!(hook.has_action());
    }

    #[test]
    fn parse_invalid_hook_reports_error_location() {
        let parsed = parse_kiro_hook(r#"{"event":"fileEdited","patterns":[}"#);
        let error = parsed.parse_error.expect("expected parse error");
        assert!(error.line > 0);
        assert!(error.column > 0);
    }
}
