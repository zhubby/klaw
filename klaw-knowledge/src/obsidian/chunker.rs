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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakPoint {
    pub byte_offset: usize,
    pub line_number: usize,
    pub score: u32,
    pub inside_code_fence: bool,
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

pub fn find_break_points(content: &str) -> Vec<BreakPoint> {
    let mut break_points = Vec::new();
    let mut inside_code_fence = false;
    let mut byte_offset = 0;

    for (line_number, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let line_end =
            byte_offset + line.len() + usize::from(byte_offset + line.len() < content.len());

        if is_code_fence(trimmed) {
            let boundary_offset = if inside_code_fence {
                line_end
            } else {
                byte_offset
            };
            break_points.push(BreakPoint {
                byte_offset: boundary_offset,
                line_number,
                score: 80,
                inside_code_fence: false,
            });
            inside_code_fence = !inside_code_fence;
            byte_offset = line_end;
            continue;
        }

        if inside_code_fence {
            break_points.push(BreakPoint {
                byte_offset,
                line_number,
                score: 1,
                inside_code_fence: true,
            });
            byte_offset = line_end;
            continue;
        }

        let score = break_point_score(trimmed);
        if score > 1 {
            break_points.push(BreakPoint {
                byte_offset,
                line_number,
                score,
                inside_code_fence,
            });
        }
        byte_offset = line_end;
    }

    break_points
}

pub fn smart_chunk(content: &str, target_tokens: usize, overlap_pct: usize) -> Vec<Chunk> {
    if content.trim().is_empty() {
        return Vec::new();
    }
    if target_tokens == 0 {
        return vec![make_chunk(content.trim())];
    }

    let target_chars = target_tokens * 4;
    if approx_tokens(content) <= target_tokens {
        return vec![make_chunk(content.trim())];
    }

    let break_points = find_break_points(content);
    let mut chunks = Vec::new();
    let overlap_chars = target_chars * overlap_pct / 100;
    let mut start_offset = 0;

    while start_offset < content.len() {
        start_offset = snap_to_char_boundary(content, start_offset);
        let remaining = &content[start_offset..];
        if remaining.trim().is_empty() {
            break;
        }
        if approx_tokens(remaining) <= target_tokens {
            chunks.push(make_chunk(remaining.trim()));
            break;
        }

        let ideal_end = start_offset + target_chars;
        let cut_offset = break_points
            .iter()
            .filter(|point| {
                point.byte_offset > start_offset
                    && point.byte_offset <= start_offset + target_chars * 2
                    && !point.inside_code_fence
            })
            .max_by(|left, right| {
                weighted_score(left, ideal_end)
                    .partial_cmp(&weighted_score(right, ideal_end))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|point| point.byte_offset)
            .unwrap_or_else(|| fallback_cut_offset(content, start_offset, target_chars));

        let cut_offset = snap_to_char_boundary(content, cut_offset)
            .max(start_offset + 1)
            .min(content.len());
        let chunk_text = content[start_offset..cut_offset].trim();
        if !chunk_text.is_empty() {
            chunks.push(make_chunk(chunk_text));
        }
        if cut_offset >= content.len() {
            break;
        }
        start_offset = next_start_offset(content, start_offset, cut_offset, overlap_chars);
    }

    chunks
}

pub fn split_oversized_chunks(
    chunks: Vec<Chunk>,
    token_count: &dyn Fn(&str) -> usize,
    max_tokens: usize,
    overlap_tokens: usize,
) -> Vec<Chunk> {
    chunks
        .into_iter()
        .flat_map(|chunk| {
            if token_count(&chunk.text) <= max_tokens {
                return vec![chunk];
            }
            split_oversized_chunk(chunk, token_count, max_tokens, overlap_tokens)
        })
        .collect()
}

fn split_oversized_chunk(
    chunk: Chunk,
    token_count: &dyn Fn(&str) -> usize,
    max_tokens: usize,
    overlap_tokens: usize,
) -> Vec<Chunk> {
    let mut result = Vec::new();
    let mut current = String::new();
    for segment in split_sentence_segments(&chunk.text) {
        let candidate = format!("{current}{segment}");
        if !current.is_empty() && token_count(&candidate) > max_tokens {
            result.push(current.clone());
            let overlap = build_token_overlap(&current, token_count, overlap_tokens);
            current = format!("{overlap}{segment}");
        } else {
            current = candidate;
        }
    }
    if !current.trim().is_empty() {
        result.push(current);
    }

    result
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let heading = if index == 0 {
                chunk.heading.clone()
            } else {
                chunk
                    .heading
                    .as_ref()
                    .map(|heading| format!("{heading} (cont.)"))
            };
            let text = text.trim().to_string();
            Chunk {
                heading,
                snippet: make_snippet(&text),
                text,
            }
        })
        .collect()
}

fn break_point_score(trimmed: &str) -> u32 {
    if trimmed.starts_with("# ") {
        100
    } else if trimmed.starts_with("## ") {
        90
    } else if trimmed.starts_with("### ") {
        80
    } else if trimmed.starts_with("#### ") {
        70
    } else if trimmed.starts_with("##### ") {
        60
    } else if trimmed.starts_with("###### ") {
        50
    } else if is_thematic_break(trimmed) {
        60
    } else if trimmed.is_empty() {
        20
    } else if is_list_item(trimmed) {
        5
    } else {
        1
    }
}

fn is_code_fence(trimmed: &str) -> bool {
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn is_thematic_break(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    trimmed.chars().all(|ch| ch == first || ch == ' ')
        && trimmed.chars().filter(|ch| *ch == first).count() >= 3
}

fn is_list_item(trimmed: &str) -> bool {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return true;
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_digit() {
        return false;
    }
    for ch in chars {
        if ch.is_ascii_digit() {
            continue;
        }
        return ch == '.' || ch == ')';
    }
    false
}

fn approx_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn weighted_score(point: &BreakPoint, ideal_offset: usize) -> f64 {
    let distance = (point.byte_offset as f64 - ideal_offset as f64).abs();
    let distance_factor = 1.0 / (1.0 + distance / 500.0);
    point.score as f64 * distance_factor
}

fn fallback_cut_offset(content: &str, start_offset: usize, target_chars: usize) -> usize {
    let cut = snap_to_char_boundary(content, (start_offset + target_chars).min(content.len()));
    content[start_offset..cut]
        .rfind('\n')
        .map(|position| start_offset + position + 1)
        .filter(|offset| *offset > start_offset)
        .unwrap_or(cut)
        .max(start_offset + 1)
        .min(content.len())
}

fn snap_to_char_boundary(content: &str, offset: usize) -> usize {
    let mut offset = offset.min(content.len());
    while offset < content.len() && !content.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

fn next_start_offset(
    content: &str,
    start_offset: usize,
    cut_offset: usize,
    overlap_chars: usize,
) -> usize {
    if overlap_chars == 0 || cut_offset <= overlap_chars {
        return cut_offset;
    }
    let candidate =
        snap_to_char_boundary(content, cut_offset - overlap_chars).max(start_offset + 1);
    if is_inside_code_fence_at(content, candidate) {
        cut_offset
    } else {
        candidate
    }
}

fn is_inside_code_fence_at(content: &str, offset: usize) -> bool {
    let mut inside = false;
    let mut byte_offset = 0;
    for line in content.lines() {
        if byte_offset >= offset {
            break;
        }
        if is_code_fence(line.trim()) {
            inside = !inside;
        }
        byte_offset += line.len() + usize::from(byte_offset + line.len() < content.len());
    }
    inside
}

fn split_sentence_segments(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        current.push(chars[index]);
        if chars[index] == '\n' {
            segments.push(current.clone());
            current.clear();
        } else if chars[index] == '.' && chars.get(index + 1) == Some(&' ') {
            current.push(' ');
            index += 1;
            segments.push(current.clone());
            current.clear();
        }
        index += 1;
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

fn build_token_overlap(
    text: &str,
    token_count: &dyn Fn(&str) -> usize,
    overlap_tokens: usize,
) -> String {
    if overlap_tokens == 0 {
        return String::new();
    }
    let mut overlap = String::new();
    for word in text.split_whitespace().rev() {
        let candidate = if overlap.is_empty() {
            word.to_string()
        } else {
            format!("{word} {overlap}")
        };
        if token_count(&candidate) > overlap_tokens {
            break;
        }
        overlap = candidate;
    }
    if overlap.is_empty() {
        overlap
    } else {
        format!("{overlap} ")
    }
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

    #[test]
    fn scores_semantic_break_points() {
        let content = "# One\n\n## Two\n---\n```rust\nlet value = 1;\n```\n###### Six\n";
        let break_points = find_break_points(content);
        let scores: Vec<u32> = break_points
            .iter()
            .filter(|point| !point.inside_code_fence)
            .map(|point| point.score)
            .collect();

        assert!(scores.contains(&100));
        assert!(scores.contains(&90));
        assert!(scores.contains(&80));
        assert!(scores.contains(&60));
        assert!(scores.contains(&50));
        assert!(scores.contains(&20));
        assert!(break_points.iter().any(|point| point.inside_code_fence));
    }

    #[test]
    fn prefers_thematic_breaks_near_target_size() {
        let content = format!(
            "# Intro\n{}\n---\nTail\n{}",
            "intro ".repeat(32),
            "tail ".repeat(32)
        );

        let chunks = smart_chunk(&content, 48, 0);

        assert!(chunks.len() > 1);
        assert!(chunks[0].text.trim_end().ends_with("intro"));
        assert!(chunks[1].text.starts_with("---\nTail"));
    }

    #[test]
    fn splits_oversized_chunks_on_sentence_boundaries() {
        let chunks = vec![make_chunk(&format!(
            "# Intro\n{}",
            "Sentence one. Sentence two. Sentence three. ".repeat(20)
        ))];

        let split = split_oversized_chunks(chunks, &|text| text.len().div_ceil(4), 40, 5);

        assert!(split.len() > 1);
        assert!(split.iter().all(|chunk| !chunk.text.trim().is_empty()));
        assert!(split.iter().skip(1).all(|chunk| {
            chunk
                .heading
                .as_deref()
                .is_none_or(|heading| heading.ends_with("(cont.)"))
        }));
    }
}
