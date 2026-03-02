//! Kiro power file schema helpers.
//!
//! Covers `POWER.md` frontmatter + body parsing.
#![allow(dead_code)]

use crate::parsers::frontmatter::split_frontmatter;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// YAML/frontmatter parse error with optional location metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

/// Parsed Kiro power markdown.
#[derive(Debug, Clone)]
pub struct ParsedKiroPower {
    pub frontmatter: Option<KiroPowerFrontmatter>,
    pub body: String,
    pub has_frontmatter: bool,
    pub has_closing_frontmatter: bool,
    pub parse_error: Option<ParseError>,
}

/// Kiro POWER.md frontmatter schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroPowerFrontmatter {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub author: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Parse POWER.md content into structured frontmatter + body.
pub fn parse_kiro_power(content: &str) -> ParsedKiroPower {
    let parts = split_frontmatter(content);

    if !parts.has_frontmatter {
        return ParsedKiroPower {
            frontmatter: None,
            body: content.to_string(),
            has_frontmatter: false,
            has_closing_frontmatter: false,
            parse_error: None,
        };
    }

    if !parts.has_closing {
        return ParsedKiroPower {
            frontmatter: None,
            body: content.to_string(),
            has_frontmatter: true,
            has_closing_frontmatter: false,
            parse_error: Some(ParseError {
                message: "Missing closing frontmatter delimiter".to_string(),
                line: 1,
                column: 1,
            }),
        };
    }

    match serde_yaml::from_str::<KiroPowerFrontmatter>(&parts.frontmatter) {
        Ok(frontmatter) => ParsedKiroPower {
            frontmatter: Some(frontmatter),
            body: parts.body.to_string(),
            has_frontmatter: true,
            has_closing_frontmatter: true,
            parse_error: None,
        },
        Err(err) => {
            let (line, column) = err
                .location()
                .map(|loc| (loc.line(), loc.column()))
                .unwrap_or((1, 1));

            ParsedKiroPower {
                frontmatter: None,
                body: parts.body.to_string(),
                has_frontmatter: true,
                has_closing_frontmatter: true,
                parse_error: Some(ParseError {
                    message: err.to_string(),
                    line,
                    column,
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_power_frontmatter_and_body() {
        let parsed = parse_kiro_power(
            r#"---
name: test-power
description: Valid power
keywords:
  - kiro
  - power
---
# Test

Body content.
"#,
        );

        assert!(parsed.parse_error.is_none());
        assert!(parsed.has_frontmatter);
        assert!(parsed.has_closing_frontmatter);
        assert!(parsed.body.contains("Body content"));
        assert_eq!(
            parsed.frontmatter.and_then(|fm| fm.name),
            Some("test-power".to_string())
        );
    }

    #[test]
    fn parse_power_without_frontmatter_is_supported() {
        let parsed = parse_kiro_power("# No frontmatter\n\nBody");
        assert!(!parsed.has_frontmatter);
        assert!(parsed.parse_error.is_none());
    }

    #[test]
    fn parse_power_with_invalid_yaml_reports_location() {
        let parsed = parse_kiro_power(
            r#"---
name:
  - bad
description: still present
keywords: [ok
---
Body
"#,
        );

        let error = parsed.parse_error.expect("expected parse error");
        assert!(error.line > 0);
        assert!(error.column > 0);
    }
}
