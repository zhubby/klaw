use regex::Regex;
use serde::{Deserialize, Serialize};
use time::Date;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedNote {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub wikilinks: Vec<String>,
    pub inline_tags: Vec<String>,
    pub note_date: Option<String>,
    pub body: String,
}

pub fn parse_note(content: &str) -> ParsedNote {
    let (frontmatter, body) = split_frontmatter(content);
    let tags = parse_list_field(frontmatter, "tags");
    let aliases = parse_list_field(frontmatter, "aliases");
    let title = parse_scalar_field(frontmatter, "title").or_else(|| first_heading_title(body));
    let note_date = parse_scalar_field(frontmatter, "date").filter(|value| {
        time::format_description::parse("[year]-[month]-[day]")
            .ok()
            .and_then(|format| Date::parse(value, &format).ok())
            .is_some()
    });
    let wikilinks = parse_wikilinks(body);
    let inline_tags = parse_inline_tags(body);

    ParsedNote {
        title,
        tags,
        aliases,
        wikilinks,
        inline_tags,
        note_date,
        body: body.trim().to_string(),
    }
}

fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (None, content);
    }
    let after_open = trimmed[3..].strip_prefix('\n').unwrap_or(&trimmed[3..]);
    if let Some(end) = after_open.find("\n---") {
        let frontmatter = &after_open[..end];
        let rest = &after_open[end + 4..];
        let body = rest.strip_prefix('\n').unwrap_or(rest);
        (Some(frontmatter), body)
    } else {
        (None, content)
    }
}

fn parse_list_field(frontmatter: Option<&str>, key: &str) -> Vec<String> {
    let Some(frontmatter) = frontmatter else {
        return Vec::new();
    };
    let lines: Vec<&str> = frontmatter.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let prefix = format!("{key}:");
        if !trimmed.starts_with(&prefix) {
            continue;
        }
        let rest = trimmed[prefix.len()..].trim();
        if rest.starts_with('[') && rest.ends_with(']') {
            return rest[1..rest.len() - 1]
                .split(',')
                .map(|item| item.trim().trim_matches('"').trim_matches('\''))
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect();
        }
        if !rest.is_empty() {
            return vec![rest.trim_matches('"').trim_matches('\'').to_string()];
        }
        let mut values = Vec::new();
        for next in lines.iter().skip(idx + 1) {
            let next = next.trim();
            if let Some(value) = next.strip_prefix("- ") {
                values.push(
                    value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string(),
                );
            } else if next.is_empty() {
                continue;
            } else {
                break;
            }
        }
        return values;
    }
    Vec::new()
}

fn parse_scalar_field(frontmatter: Option<&str>, key: &str) -> Option<String> {
    let frontmatter = frontmatter?;
    let prefix = format!("{key}:");
    frontmatter.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.trim_matches('"').trim_matches('\'').to_string())
    })
}

fn parse_wikilinks(body: &str) -> Vec<String> {
    let regex = Regex::new(r"\[\[([^\]|#]+)(?:#[^\]|]+)?(?:\|[^\]]+)?\]\]").expect("valid regex");
    regex
        .captures_iter(body)
        .filter_map(|caps| caps.get(1).map(|m| m.as_str().trim().to_string()))
        .collect()
}

fn parse_inline_tags(body: &str) -> Vec<String> {
    let regex = Regex::new(r"(?P<tag>#[A-Za-z0-9_\-/]+)").expect("valid regex");
    regex
        .captures_iter(body)
        .filter_map(|caps| {
            caps.name("tag")
                .map(|m| m.as_str().trim_start_matches('#').to_string())
        })
        .collect()
}

fn first_heading_title(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("# ")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_lists_and_scalar_fields() {
        let note = parse_note(
            "---\n\
title: Rust Async\n\
tags: [rust, async]\n\
aliases:\n\
  - futures rust\n\
date: 2026-04-24\n\
---\n\
# Rust Async\nBody",
        );
        assert_eq!(note.title.as_deref(), Some("Rust Async"));
        assert_eq!(note.tags, vec!["rust".to_string(), "async".to_string()]);
        assert_eq!(note.aliases, vec!["futures rust".to_string()]);
        assert_eq!(note.note_date.as_deref(), Some("2026-04-24"));
        assert_eq!(note.body, "# Rust Async\nBody");
    }

    #[test]
    fn parses_wikilinks_and_inline_tags() {
        let note = parse_note("See [[Other Note|display]] and [[Topic/Sub]]. #rust #async");
        assert_eq!(
            note.wikilinks,
            vec!["Other Note".to_string(), "Topic/Sub".to_string()]
        );
        assert_eq!(
            note.inline_tags,
            vec!["rust".to_string(), "async".to_string()]
        );
    }
}
