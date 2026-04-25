use serde::{Deserialize, Serialize};

use super::parser::parse_note;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chunk {
    pub heading: Option<String>,
    pub text: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedMarkdown {
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub chunks: Vec<Chunk>,
}

pub fn chunk_markdown(content: &str) -> ParsedMarkdown {
    let note = parse_note(content);
    let chunks = smart_chunk(&note.body, 512, 15);
    ParsedMarkdown {
        tags: note.tags,
        aliases: note.aliases,
        chunks,
    }
}

pub fn smart_chunk(content: &str, target_tokens: usize, overlap_pct: usize) -> Vec<Chunk> {
    if content.trim().is_empty() {
        return Vec::new();
    }
    let target_chars = target_tokens * 4;
    if content.len() <= target_chars {
        return vec![make_chunk(content.trim())];
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut inside_code_fence = false;
    let overlap_chars = target_chars * overlap_pct / 100;

    for line in lines {
        if line.trim_start().starts_with("```") {
            inside_code_fence = !inside_code_fence;
        }
        let candidate = if current.is_empty() {
            line.to_string()
        } else {
            format!("{current}\n{line}")
        };
        let is_break_line = line.starts_with('#') || line.trim().is_empty();
        if !inside_code_fence
            && candidate.len() > target_chars
            && is_break_line
            && !current.is_empty()
        {
            chunks.push(make_chunk(current.trim()));
            current = take_overlap(&current, overlap_chars);
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        } else {
            current = candidate;
        }
    }

    if !current.trim().is_empty() {
        chunks.push(make_chunk(current.trim()));
    }
    chunks
}

fn take_overlap(text: &str, overlap_chars: usize) -> String {
    if overlap_chars == 0 || text.len() <= overlap_chars {
        return String::new();
    }
    let start = text
        .char_indices()
        .rev()
        .nth(overlap_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    text[start..].trim().to_string()
}

fn make_chunk(text: &str) -> Chunk {
    Chunk {
        heading: extract_heading(text),
        text: text.to_string(),
        snippet: make_snippet(text),
    }
}

fn extract_heading(text: &str) -> Option<String> {
    text.lines()
        .find(|line| line.trim().starts_with('#'))
        .map(str::to_string)
}

fn make_snippet(text: &str) -> String {
    if text.chars().count() > 200 {
        text.chars().take(200).collect::<String>() + "..."
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_small_markdown_as_single_chunk() {
        let parsed =
            chunk_markdown("---\ntags: [rust]\naliases: [async]\n---\n# Title\nShort body");
        assert_eq!(parsed.tags, vec!["rust".to_string()]);
        assert_eq!(parsed.aliases, vec!["async".to_string()]);
        assert_eq!(parsed.chunks.len(), 1);
        assert_eq!(parsed.chunks[0].heading.as_deref(), Some("# Title"));
    }

    #[test]
    fn splits_large_markdown_on_headings() {
        let mut content = String::from("# Intro\n");
        for idx in 0..40 {
            content.push_str(&format!("\n## Section {idx}\n{}\n", "content ".repeat(40)));
        }
        let chunks = smart_chunk(&content, 64, 10);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| !chunk.snippet.is_empty()));
    }

    #[test]
    fn avoids_splitting_mid_code_fence() {
        let content = format!(
            "# Intro\n{}\n```rust\n{}\n```\n## Tail\n{}",
            "text ".repeat(80),
            "let value = 1;\n".repeat(40),
            "tail ".repeat(60)
        );
        let chunks = smart_chunk(&content, 64, 10);
        assert!(chunks.iter().any(|chunk| chunk.text.contains("```rust")));
    }
}
