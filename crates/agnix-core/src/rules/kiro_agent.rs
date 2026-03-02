//! Kiro agent validation rules (KR-AG-006 to KR-AG-007).
//!
//! Validates cross-agent invocation references in `.kiro/agents/*.json`:
//! - KR-AG-006: Prompt references a non-existent subagent.
//! - KR-AG-007: Invoking agent has a broader tool scope than referenced subagent.

use crate::{
    config::LintConfig,
    diagnostics::Diagnostic,
    rules::{Validator, ValidatorMetadata},
};
use rust_i18n::t;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const RULE_IDS: &[&str] = &["KR-AG-006", "KR-AG-007"];
const MAX_PROJECT_SEARCH_DEPTH: usize = 10;

#[derive(Debug, Clone)]
struct AgentInfo {
    tools: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentMention {
    name: String,
    byte_offset: usize,
}

fn normalize_agent_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn mention_regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"(^|[^A-Za-z0-9_@])@([A-Za-z][A-Za-z0-9_-]{0,63})")
            .expect("mention regex must compile")
    })
}

fn extract_agent_mentions(content: &str) -> Vec<AgentMention> {
    let mut seen = HashSet::new();
    let mut mentions = Vec::new();

    for captures in mention_regex().captures_iter(content) {
        let Some(name_match) = captures.get(2) else {
            continue;
        };

        let normalized = normalize_agent_name(name_match.as_str());
        if normalized.is_empty() {
            continue;
        }

        // Keep the first occurrence for stable diagnostics.
        if seen.insert(normalized.clone()) {
            mentions.push(AgentMention {
                name: normalized,
                byte_offset: name_match.start().saturating_sub(1), // include '@'
            });
        }
    }

    mentions
}

fn extract_tools(value: &Value) -> HashSet<String> {
    fn parse_tool_array(value: Option<&Value>) -> HashSet<String> {
        let mut tools = HashSet::new();
        let Some(array) = value.and_then(Value::as_array) else {
            return tools;
        };

        for item in array {
            if let Some(tool) = item.as_str() {
                let normalized = tool.trim().to_ascii_lowercase();
                if !normalized.is_empty() {
                    tools.insert(normalized);
                }
            }
        }
        tools
    }

    // Prefer allowedTools as the effective policy surface when present.
    let allowed = parse_tool_array(value.get("allowedTools"));
    if !allowed.is_empty() {
        return allowed;
    }

    parse_tool_array(value.get("tools"))
}

fn line_col_at_offset(content: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 0usize;

    for (idx, ch) in content.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    (line, col)
}

fn find_kiro_agents_dir(path: &Path, config: &LintConfig) -> Option<PathBuf> {
    let fs = config.fs();

    if let Some(parent) = path.parent() {
        let parent_name = parent.file_name().and_then(|n| n.to_str());
        let grandparent_name = parent
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str());
        if parent_name == Some("agents") && grandparent_name == Some(".kiro") {
            return Some(parent.to_path_buf());
        }
    }

    let mut current = path.parent();
    let mut depth = 0usize;

    while let Some(dir) = current {
        if depth >= MAX_PROJECT_SEARCH_DEPTH {
            break;
        }

        let candidate = dir.join(".kiro").join("agents");
        if fs.is_dir(&candidate) {
            return Some(candidate);
        }

        current = dir.parent();
        depth += 1;
    }

    None
}

fn load_agent_index(agents_dir: &Path, config: &LintConfig) -> HashMap<String, AgentInfo> {
    let fs = config.fs();
    let Ok(mut entries) = fs.read_dir(agents_dir) else {
        return HashMap::new();
    };

    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let mut index: HashMap<String, AgentInfo> = HashMap::new();

    for entry in entries {
        if !entry.metadata.is_file {
            continue;
        }

        let is_json = entry
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
        if !is_json {
            continue;
        }

        let Ok(raw) = fs.read_to_string(&entry.path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };

        let explicit_name = value.get("name").and_then(Value::as_str);
        let fallback_name = entry.path.file_stem().and_then(|stem| stem.to_str());
        let Some(name) = explicit_name.or(fallback_name) else {
            continue;
        };

        let normalized_name = normalize_agent_name(name);
        if normalized_name.is_empty() {
            continue;
        }

        // Keep first observed definition for deterministic conflict handling.
        index.entry(normalized_name).or_insert_with(|| AgentInfo {
            tools: extract_tools(&value),
        });
    }

    index
}

pub struct KiroAgentValidator;

impl Validator for KiroAgentValidator {
    fn metadata(&self) -> ValidatorMetadata {
        ValidatorMetadata {
            name: self.name(),
            rule_ids: RULE_IDS,
        }
    }

    fn validate(&self, path: &Path, content: &str, config: &LintConfig) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let check_missing_reference = config.is_rule_enabled("KR-AG-006");
        let check_tool_scope = config.is_rule_enabled("KR-AG-007");
        if !check_missing_reference && !check_tool_scope {
            return diagnostics;
        }

        let mentions = extract_agent_mentions(content);
        if mentions.is_empty() {
            return diagnostics;
        }

        let Ok(current_agent) = serde_json::from_str::<Value>(content) else {
            return diagnostics;
        };

        let current_name = current_agent
            .get("name")
            .and_then(Value::as_str)
            .map(normalize_agent_name)
            .or_else(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(normalize_agent_name)
            });
        let current_tools = extract_tools(&current_agent);

        let Some(agents_dir) = find_kiro_agents_dir(path, config) else {
            return diagnostics;
        };

        let known_agents = load_agent_index(&agents_dir, config);
        if known_agents.is_empty() {
            return diagnostics;
        }

        for mention in mentions {
            if current_name.as_ref() == Some(&mention.name) {
                continue;
            }

            let (line, col) = line_col_at_offset(content, mention.byte_offset);
            let display_name = mention.name.as_str();

            let Some(referenced_agent) = known_agents.get(&mention.name) else {
                if check_missing_reference {
                    diagnostics.push(
                        Diagnostic::warning(
                            path.to_path_buf(),
                            line,
                            col,
                            "KR-AG-006",
                            t!("rules.kr_ag_006.message", agent = display_name),
                        )
                        .with_suggestion(t!("rules.kr_ag_006.suggestion", agent = display_name)),
                    );
                }
                continue;
            };

            if !check_tool_scope || current_tools.is_empty() || referenced_agent.tools.is_empty() {
                continue;
            }

            let mut extra_tools: Vec<String> = current_tools
                .difference(&referenced_agent.tools)
                .cloned()
                .collect();
            if extra_tools.is_empty() {
                continue;
            }

            extra_tools.sort();
            diagnostics.push(
                Diagnostic::warning(
                    path.to_path_buf(),
                    line,
                    col,
                    "KR-AG-007",
                    t!(
                        "rules.kr_ag_007.message",
                        agent = display_name,
                        extra_tools = extra_tools.join(", ")
                    ),
                )
                .with_suggestion(t!("rules.kr_ag_007.suggestion", agent = display_name)),
            );
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_agent(path: &Path, content: &str) {
        fs::write(path, content).unwrap_or_else(|e| {
            panic!("Failed writing {}: {}", path.display(), e);
        });
    }

    fn validate(path: &Path) -> Vec<Diagnostic> {
        let validator = KiroAgentValidator;
        let content = fs::read_to_string(path).unwrap();
        validator.validate(path, &content, &LintConfig::default())
    }

    #[test]
    fn test_kr_ag_006_reports_unknown_subagent_reference() {
        let temp = tempfile::TempDir::new().unwrap();
        let agents_dir = temp.path().join(".kiro").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        let orchestrator = agents_dir.join("orchestrator.json");
        write_agent(
            &orchestrator,
            r#"{
  "name": "orchestrator",
  "prompt": "Delegate this to @research-agent"
}"#,
        );

        let diagnostics = validate(&orchestrator);
        let kr_ag_006: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.rule == "KR-AG-006")
            .collect();

        assert_eq!(kr_ag_006.len(), 1);
        assert!(!kr_ag_006[0].message.trim().is_empty());
    }

    #[test]
    fn test_kr_ag_006_skips_when_reference_exists() {
        let temp = tempfile::TempDir::new().unwrap();
        let agents_dir = temp.path().join(".kiro").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        let orchestrator = agents_dir.join("orchestrator.json");
        let worker = agents_dir.join("research-agent.json");

        write_agent(
            &orchestrator,
            r#"{
  "name": "orchestrator",
  "prompt": "Delegate this to @research-agent"
}"#,
        );
        write_agent(
            &worker,
            r#"{
  "name": "research-agent",
  "tools": ["readFiles"]
}"#,
        );

        let diagnostics = validate(&orchestrator);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.rule != "KR-AG-006"),
            "KR-AG-006 should not fire when subagent exists: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_kr_ag_007_reports_broader_parent_tools() {
        let temp = tempfile::TempDir::new().unwrap();
        let agents_dir = temp.path().join(".kiro").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        let orchestrator = agents_dir.join("orchestrator.json");
        let reviewer = agents_dir.join("reviewer.json");

        write_agent(
            &orchestrator,
            r#"{
  "name": "orchestrator",
  "allowedTools": ["readFiles", "runShellCommand"],
  "prompt": "Use @reviewer for checks"
}"#,
        );
        write_agent(
            &reviewer,
            r#"{
  "name": "reviewer",
  "allowedTools": ["readFiles"]
}"#,
        );

        let diagnostics = validate(&orchestrator);
        let kr_ag_007: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.rule == "KR-AG-007")
            .collect();

        assert_eq!(kr_ag_007.len(), 1);
        assert!(!kr_ag_007[0].message.trim().is_empty());
    }

    #[test]
    fn test_kr_ag_007_skips_when_tool_scope_not_broader() {
        let temp = tempfile::TempDir::new().unwrap();
        let agents_dir = temp.path().join(".kiro").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        let orchestrator = agents_dir.join("orchestrator.json");
        let reviewer = agents_dir.join("reviewer.json");

        write_agent(
            &orchestrator,
            r#"{
  "name": "orchestrator",
  "tools": ["readFiles"],
  "prompt": "Use @reviewer for checks"
}"#,
        );
        write_agent(
            &reviewer,
            r#"{
  "name": "reviewer",
  "tools": ["readFiles", "listDirectory"]
}"#,
        );

        let diagnostics = validate(&orchestrator);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.rule != "KR-AG-007"),
            "KR-AG-007 should not fire when parent tools are not broader: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_rules_can_be_disabled_individually() {
        let temp = tempfile::TempDir::new().unwrap();
        let agents_dir = temp.path().join(".kiro").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        let orchestrator = agents_dir.join("orchestrator.json");
        let reviewer = agents_dir.join("reviewer.json");

        write_agent(
            &orchestrator,
            r#"{
  "name": "orchestrator",
  "allowedTools": ["readFiles", "runShellCommand"],
  "prompt": "Use @missing-agent and @reviewer"
}"#,
        );
        write_agent(
            &reviewer,
            r#"{
  "name": "reviewer",
  "allowedTools": ["readFiles"]
}"#,
        );

        let validator = KiroAgentValidator;
        let content = fs::read_to_string(&orchestrator).unwrap();

        let mut config = LintConfig::default();
        config.rules_mut().disabled_rules = vec!["KR-AG-006".to_string()];
        let diagnostics = validator.validate(&orchestrator, &content, &config);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.rule != "KR-AG-006")
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == "KR-AG-007")
        );
    }

    #[test]
    fn test_metadata_lists_kr_ag_rules() {
        let validator = KiroAgentValidator;
        let metadata = validator.metadata();

        assert_eq!(metadata.name, "KiroAgentValidator");
        assert_eq!(metadata.rule_ids, &["KR-AG-006", "KR-AG-007"]);
    }
}
