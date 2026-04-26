use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use strsim::normalized_levenshtein;

const FUZZY_MATCH_THRESHOLD_BP: u16 = 920;
const FIRST_NAME_CONFIDENCE_BP: u16 = 650;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoteLinkTarget {
    pub path: String,
    pub title: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredLink {
    pub matched_text: String,
    pub target_path: String,
    pub target_title: String,
    pub display: Option<String>,
    pub match_type: LinkMatchType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LinkMatchType {
    ExactName,
    Alias,
    FuzzyName { confidence_bp: u16 },
    FirstName { confidence_bp: u16 },
}

impl LinkMatchType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ExactName => "exact_name",
            Self::Alias => "alias",
            Self::FuzzyName { .. } => "fuzzy_name",
            Self::FirstName { .. } => "first_name",
        }
    }

    fn priority(&self) -> u8 {
        match self {
            Self::ExactName => 0,
            Self::Alias => 1,
            Self::FuzzyName { .. } => 2,
            Self::FirstName { .. } => 3,
        }
    }

    #[must_use]
    pub fn confidence_bp(&self) -> Option<u16> {
        match self {
            Self::FuzzyName { confidence_bp } | Self::FirstName { confidence_bp } => {
                Some(*confidence_bp)
            }
            Self::ExactName | Self::Alias => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameEntry {
    pub name: String,
    pub name_lower: String,
    pub path: String,
    pub title: String,
    pub match_type: LinkMatchType,
}

pub fn build_name_index(targets: impl IntoIterator<Item = NoteLinkTarget>) -> Vec<NameEntry> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for target in targets {
        let basename = target
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&target.path)
            .trim_end_matches(".md")
            .to_string();
        push_name_entry(
            &mut entries,
            &mut seen,
            basename,
            &target,
            LinkMatchType::ExactName,
            3,
        );
        push_name_entry(
            &mut entries,
            &mut seen,
            target.title.clone(),
            &target,
            LinkMatchType::ExactName,
            3,
        );
        for alias in &target.aliases {
            push_name_entry(
                &mut entries,
                &mut seen,
                alias.clone(),
                &target,
                LinkMatchType::Alias,
                2,
            );
        }
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.name.len()));
    entries
}

pub fn discover_links(
    content: &str,
    name_index: &[NameEntry],
    people_folder: Option<&str>,
) -> Vec<DiscoveredLink> {
    let wikilink_regions = find_wikilink_regions(content);
    let protected_regions = find_protected_regions(content);
    let content_lower = content.to_ascii_lowercase();
    let content_bytes = content.as_bytes();
    let mut links = Vec::new();
    let mut claimed = Vec::new();
    let mut exact_matched_paths = HashSet::new();

    for entry in name_index {
        if !matches!(
            entry.match_type,
            LinkMatchType::ExactName | LinkMatchType::Alias
        ) {
            continue;
        }
        let mut search_from = 0;
        while let Some(relative_pos) = content_lower[search_from..].find(entry.name_lower.as_str())
        {
            let pos = search_from + relative_pos;
            let end = pos + entry.name_lower.len();
            search_from = end;

            if should_skip_match(
                content,
                content_bytes,
                pos,
                end,
                &wikilink_regions,
                &protected_regions,
                &claimed,
            ) {
                continue;
            }

            let matched_text = content[pos..end].to_string();
            let display = matches!(
                entry.match_type,
                LinkMatchType::Alias
                    | LinkMatchType::FuzzyName { .. }
                    | LinkMatchType::FirstName { .. }
            )
            .then(|| matched_text.clone());
            links.push(DiscoveredLink {
                matched_text,
                target_path: entry.path.clone(),
                target_title: entry.title.clone(),
                display,
                match_type: entry.match_type.clone(),
            });
            claimed.push((pos, end));
            exact_matched_paths.insert(entry.path.clone());
        }
    }

    let eligible: Vec<NameEntry> = name_index
        .iter()
        .filter(|entry| matches!(entry.match_type, LinkMatchType::ExactName))
        .filter(|entry| !exact_matched_paths.contains(&entry.path))
        .filter(|entry| {
            let word_count = entry.name.split_whitespace().count();
            word_count >= 2
                || people_folder.is_some_and(|folder| {
                    path_in_folder(&entry.path, folder) && entry.name.chars().count() >= 3
                })
        })
        .cloned()
        .collect();
    let mut fuzzy_excluded = claimed.clone();
    fuzzy_excluded.extend_from_slice(&wikilink_regions);
    fuzzy_excluded.extend_from_slice(&protected_regions);
    let fuzzy_matches = find_fuzzy_matches(content, &eligible, &fuzzy_excluded, people_folder);

    let mut first_name_excluded = fuzzy_excluded;
    for link in &fuzzy_matches {
        if let Some(pos) = content_lower.find(&link.matched_text.to_ascii_lowercase()) {
            first_name_excluded.push((pos, pos + link.matched_text.len()));
        }
    }
    links.extend(fuzzy_matches);

    if let Some(folder) = people_folder {
        let people_names: Vec<NameEntry> = name_index
            .iter()
            .filter(|entry| {
                path_in_folder(&entry.path, folder)
                    && matches!(entry.match_type, LinkMatchType::ExactName)
                    && !exact_matched_paths.contains(&entry.path)
            })
            .cloned()
            .collect();
        links.extend(find_first_name_matches(
            content,
            &people_names,
            &first_name_excluded,
        ));
    }

    links.sort_by(|left, right| {
        left.match_type
            .priority()
            .cmp(&right.match_type.priority())
            .then_with(|| {
                right
                    .match_type
                    .confidence_bp()
                    .unwrap_or(1000)
                    .cmp(&left.match_type.confidence_bp().unwrap_or(1000))
            })
    });
    links
}

pub(crate) fn find_fuzzy_matches(
    content: &str,
    eligible_names: &[NameEntry],
    existing_regions: &[(usize, usize)],
    people_folder: Option<&str>,
) -> Vec<DiscoveredLink> {
    let spans = word_spans(content);
    let mut results = Vec::new();

    for entry in eligible_names {
        let word_count = entry.name.split_whitespace().count();
        if word_count <= 1
            && !people_folder.is_some_and(|folder| path_in_folder(&entry.path, folder))
        {
            continue;
        }
        if spans.len() < word_count || word_count == 0 {
            continue;
        }

        for window_start in 0..=(spans.len() - word_count) {
            let window_end = window_start + word_count - 1;
            let byte_start = spans[window_start].0;
            let byte_end = spans[window_end].1;
            if overlaps_claimed(byte_start, byte_end, existing_regions) {
                continue;
            }

            let window_text = spans[window_start..=window_end]
                .iter()
                .map(|(_, _, word)| word.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            let confidence_bp =
                (normalized_levenshtein(&window_text, &entry.name_lower) * 1000.0) as u16;
            if confidence_bp < FUZZY_MATCH_THRESHOLD_BP {
                continue;
            }

            let matched_text = content[byte_start..byte_end].to_string();
            results.push(DiscoveredLink {
                matched_text: matched_text.clone(),
                target_path: entry.path.clone(),
                target_title: entry.title.clone(),
                display: Some(matched_text),
                match_type: LinkMatchType::FuzzyName { confidence_bp },
            });
            break;
        }
    }

    results
}

pub(crate) fn find_first_name_matches(
    content: &str,
    people_names: &[NameEntry],
    existing_regions: &[(usize, usize)],
) -> Vec<DiscoveredLink> {
    word_spans(content)
        .into_iter()
        .filter(|(start, end, _)| !overlaps_claimed(*start, *end, existing_regions))
        .filter_map(|(start, end, word)| {
            let word_lower = word.to_ascii_lowercase();
            let matches: Vec<&NameEntry> = people_names
                .iter()
                .filter(|entry| {
                    entry.name_lower.starts_with(&word_lower)
                        && entry.name_lower.len() > word_lower.len()
                        && entry.name_lower.as_bytes()[word_lower.len()] == b' '
                })
                .collect();
            let [entry] = matches.as_slice() else {
                return None;
            };
            let matched_text = content[start..end].to_string();
            Some(DiscoveredLink {
                matched_text: matched_text.clone(),
                target_path: entry.path.clone(),
                target_title: entry.title.clone(),
                display: Some(matched_text),
                match_type: LinkMatchType::FirstName {
                    confidence_bp: FIRST_NAME_CONFIDENCE_BP,
                },
            })
        })
        .collect()
}

pub fn find_protected_regions(content: &str) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut regions = Vec::new();
    let mut body_start = 0;

    if content.starts_with("---\n") || content.starts_with("---\r\n") {
        let after_open = if bytes.get(3) == Some(&b'\n') { 4 } else { 5 };
        if let Some(close_relative) = content[after_open..].find("\n---\n") {
            let close_end = after_open + close_relative + 5;
            regions.push((0, close_end));
            body_start = close_end;
        } else if let Some(close_relative) = content[after_open..].find("\n---\r\n") {
            let close_end = after_open + close_relative + 6;
            regions.push((0, close_end));
            body_start = close_end;
        } else if content[after_open..].ends_with("\n---") {
            regions.push((0, len));
            body_start = len;
        }
    }

    let mut index = body_start;
    while index < len {
        let at_line_start = index == body_start || bytes.get(index.wrapping_sub(1)) == Some(&b'\n');
        if at_line_start && index + 2 < len {
            let fence = bytes[index];
            if matches!(fence, b'`' | b'~')
                && bytes[index + 1] == fence
                && bytes[index + 2] == fence
            {
                let fence_start = index;
                let line_end = content[index..]
                    .find('\n')
                    .map(|position| index + position + 1)
                    .unwrap_or(len);
                let mut scan = line_end;
                let mut found_close = false;
                while scan < len {
                    let scan_at_line_start = scan == 0 || bytes.get(scan - 1) == Some(&b'\n');
                    if scan_at_line_start
                        && scan + 2 < len
                        && bytes[scan] == fence
                        && bytes[scan + 1] == fence
                        && bytes[scan + 2] == fence
                    {
                        let close_end = content[scan..]
                            .find('\n')
                            .map(|position| scan + position + 1)
                            .unwrap_or(len);
                        regions.push((fence_start, close_end));
                        index = close_end;
                        found_close = true;
                        break;
                    }
                    scan = content[scan..]
                        .find('\n')
                        .map(|position| scan + position + 1)
                        .unwrap_or(len);
                }
                if !found_close {
                    regions.push((fence_start, len));
                    index = len;
                }
                continue;
            }
        }

        if bytes[index] == b'`' {
            let start = index;
            index += 1;
            while index < len {
                if bytes[index] == b'`' {
                    regions.push((start, index + 1));
                    index += 1;
                    break;
                }
                if bytes[index] == b'\n' {
                    break;
                }
                index += 1;
            }
            continue;
        }

        index += 1;
    }

    regions
}

pub fn find_wikilink_regions(content: &str) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut regions = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'[' && bytes[index + 1] == b'[' {
            let start = index;
            let mut scan = index + 2;
            while scan + 1 < bytes.len() {
                if bytes[scan] == b']' && bytes[scan + 1] == b']' {
                    regions.push((start, scan + 2));
                    index = scan + 2;
                    break;
                }
                scan += 1;
            }
            if scan + 1 >= bytes.len() {
                index += 2;
            }
        } else {
            index += 1;
        }
    }
    regions
}

pub(crate) fn word_spans(text: &str) -> Vec<(usize, usize, String)> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if !bytes[index].is_ascii_alphanumeric() {
            index += 1;
            continue;
        }

        let start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric()
                || (bytes[index] == b'\''
                    && index + 1 < bytes.len()
                    && bytes[index + 1].is_ascii_alphanumeric()))
        {
            index += 1;
        }
        let end = index;
        let mut word = text[start..end].to_string();
        if word.ends_with("'s") || word.ends_with("'S") {
            word.truncate(word.len() - 2);
        }
        if !word.is_empty() {
            spans.push((start, end, word));
        }
    }

    spans
}

fn push_name_entry(
    entries: &mut Vec<NameEntry>,
    seen: &mut HashSet<(String, String, &'static str)>,
    name: String,
    target: &NoteLinkTarget,
    match_type: LinkMatchType,
    min_len: usize,
) {
    let name = name.trim();
    if name.chars().count() < min_len {
        return;
    }
    let name_lower = name.to_ascii_lowercase();
    if !seen.insert((target.path.clone(), name_lower.clone(), match_type.as_str())) {
        return;
    }
    entries.push(NameEntry {
        name: name.to_string(),
        name_lower,
        path: target.path.clone(),
        title: target.title.clone(),
        match_type,
    });
}

fn should_skip_match(
    content: &str,
    content_bytes: &[u8],
    pos: usize,
    end: usize,
    wikilink_regions: &[(usize, usize)],
    protected_regions: &[(usize, usize)],
    claimed: &[(usize, usize)],
) -> bool {
    inside_region(pos, end, wikilink_regions)
        || inside_region(pos, end, protected_regions)
        || overlaps_claimed(pos, end, claimed)
        || !is_word_boundary(content_bytes, pos)
        || !is_word_boundary_after(content_bytes, end)
        || followed_by_file_extension(content_bytes, end)
        || is_date_pattern(&content[pos..end])
}

fn inside_region(pos: usize, end: usize, regions: &[(usize, usize)]) -> bool {
    regions
        .iter()
        .any(|(region_start, region_end)| pos >= *region_start && end <= *region_end)
}

fn overlaps_claimed(pos: usize, end: usize, claimed: &[(usize, usize)]) -> bool {
    claimed
        .iter()
        .any(|(claimed_start, claimed_end)| pos < *claimed_end && end > *claimed_start)
}

fn is_word_boundary(content: &[u8], pos: usize) -> bool {
    pos == 0 || !content[pos - 1].is_ascii_alphanumeric() && content[pos - 1] != b'_'
}

fn is_word_boundary_after(content: &[u8], end: usize) -> bool {
    end >= content.len() || !content[end].is_ascii_alphanumeric() && content[end] != b'_'
}

fn followed_by_file_extension(content: &[u8], end: usize) -> bool {
    if content.get(end) != Some(&b'.') {
        return false;
    }
    let mut index = end + 1;
    let mut extension_len = 0;
    while index < content.len() && content[index].is_ascii_alphanumeric() {
        extension_len += 1;
        index += 1;
    }
    (1..=6).contains(&extension_len)
}

fn is_date_pattern(text: &str) -> bool {
    let bytes = text.trim().as_bytes();
    bytes.len() == 10
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn path_in_folder(path: &str, folder: &str) -> bool {
    let folder = folder.trim_matches('/');
    path == folder
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
        || path.contains(&format!("/{folder}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_index() -> Vec<NameEntry> {
        build_name_index(vec![
            NoteLinkTarget {
                path: "People/Steve Barbera.md".to_string(),
                title: "Steve Barbera".to_string(),
                aliases: Vec::new(),
            },
            NoteLinkTarget {
                path: "Research/Reciprocal Rank Fusion.md".to_string(),
                title: "Reciprocal Rank Fusion".to_string(),
                aliases: vec!["RRF".to_string()],
            },
            NoteLinkTarget {
                path: "People/Alex Chen.md".to_string(),
                title: "Alex Chen".to_string(),
                aliases: Vec::new(),
            },
            NoteLinkTarget {
                path: "Research/Large Language Models.md".to_string(),
                title: "Large Language Models".to_string(),
                aliases: Vec::new(),
            },
        ])
    }

    #[test]
    fn discovers_exact_alias_fuzzy_and_first_name_links() {
        let links = discover_links(
            "Steve Barbera explained RRF after Larg Language Models met Alex.",
            &name_index(),
            Some("People"),
        );

        assert!(links.iter().any(|link| {
            link.matched_text == "Steve Barbera"
                && link.target_path == "People/Steve Barbera.md"
                && link.match_type == LinkMatchType::ExactName
        }));
        assert!(links.iter().any(|link| {
            link.matched_text == "RRF"
                && link.target_path == "Research/Reciprocal Rank Fusion.md"
                && link.match_type == LinkMatchType::Alias
        }));
        assert!(links.iter().any(|link| {
            link.matched_text == "Larg Language Models"
                && matches!(link.match_type, LinkMatchType::FuzzyName { confidence_bp } if confidence_bp >= 920)
        }));
        assert!(links.iter().any(|link| {
            link.matched_text == "Alex"
                && matches!(
                    link.match_type,
                    LinkMatchType::FirstName { confidence_bp: 650 }
                )
        }));
    }

    #[test]
    fn skips_existing_wikilinks_and_protected_regions() {
        let content = "---\naliases: [RRF]\n---\n\
            [[Steve Barbera]]\n\
            `RRF`\n\
            ```rust\n\
            let note = \"Reciprocal Rank Fusion\";\n\
            ```\n\
            Steve Barbera outside";

        let links = discover_links(content, &name_index(), Some("People"));

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].matched_text, "Steve Barbera");
    }

    #[test]
    fn fuzzy_matching_respects_threshold() {
        let entries = name_index();

        let links = find_fuzzy_matches(
            "We discussed Stovo Borbora yesterday.",
            &entries,
            &[],
            Some("People"),
        );

        assert!(links.is_empty());
    }

    #[test]
    fn first_name_matching_requires_unique_people_name() {
        let mut entries = name_index();
        entries.extend(build_name_index(vec![NoteLinkTarget {
            path: "People/Alex Rivera.md".to_string(),
            title: "Alex Rivera".to_string(),
            aliases: Vec::new(),
        }]));

        let links = discover_links("Alex joined later.", &entries, Some("People"));

        assert!(links.is_empty());
    }

    #[test]
    fn first_name_matching_accepts_nested_people_folder() {
        let entries = build_name_index(vec![NoteLinkTarget {
            path: "03-Resources/People/Sam Rivera.md".to_string(),
            title: "Sam Rivera".to_string(),
            aliases: Vec::new(),
        }]);

        let links = discover_links("Sam wrote the note.", &entries, Some("People"));

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_path, "03-Resources/People/Sam Rivera.md");
        assert!(matches!(
            links[0].match_type,
            LinkMatchType::FirstName { confidence_bp: 650 }
        ));
    }
}
