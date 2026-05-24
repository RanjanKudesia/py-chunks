// Re-export shared split_sentences so pdf submodules keep the same import path.
pub use super::super::shared::split_sentences;

pub const MAX_CHUNK_CHARS: usize = 1200;
pub const MIN_CHUNK_CHARS: usize = 350;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    PlainParagraph,
    HeadingSection,
    BulletNumberedList,
    Table,
    LongSingleParagraph,
    ShortDisconnectedParagraph,
}

#[derive(Debug, Clone)]
pub struct Paragraph {
    pub text: String,
    pub is_heading: bool,
    pub kind: ParagraphKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphKind {
    Plain,
    Heading,
    BulletList,
    Table,
}

#[derive(Debug)]
pub struct ChunkUnit {
    pub heading: Option<String>,
    pub parts: Vec<String>,
}

impl ContentType {
    pub fn as_str(self) -> &'static str {
        match self {
            ContentType::PlainParagraph => "plain_paragraph",
            ContentType::HeadingSection => "heading",
            ContentType::BulletNumberedList => "bullet_list",
            ContentType::Table => "table",
            ContentType::LongSingleParagraph => "long_single_paragraph",
            ContentType::ShortDisconnectedParagraph => "short_disconnected_paragraph",
        }
    }
}

pub fn split_raw_blocks(text: &str) -> Vec<Vec<String>> {
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for line in text.lines() {
        let t = line.trim().to_string();
        if t.is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(t);
        }
    }

    if !current.is_empty() {
        blocks.push(current);
    }

    blocks
}

pub fn build_paragraph(lines: Vec<String>) -> Option<Paragraph> {
    if lines.is_empty() {
        return None;
    }

    let is_bullet = is_bullet_line(&lines[0]) || is_numbered_line(&lines[0]);
    let is_table = !is_bullet && is_table_block(&lines);
    let joined = lines.join(" ");
    let is_heading = !is_table && !is_bullet && is_heading_block(&joined, &lines);

    let text = if is_table {
        lines
            .iter()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    } else if is_bullet {
        merge_bullet_lines(&lines).join("\n")
    } else if is_heading {
        lines.join("\n")
    } else {
        lines.join(" ")
    };

    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }

    let kind = if is_table {
        ParagraphKind::Table
    } else if is_bullet {
        ParagraphKind::BulletList
    } else if is_heading {
        ParagraphKind::Heading
    } else {
        ParagraphKind::Plain
    };

    Some(Paragraph {
        text,
        is_heading,
        kind,
    })
}

pub fn build_units(paragraphs: Vec<Paragraph>) -> Vec<ChunkUnit> {
    let mut units: Vec<ChunkUnit> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_parts: Vec<String> = Vec::new();

    for para in &paragraphs {
        let text = para.text.clone();
        if text.is_empty() {
            continue;
        }

        if para.is_heading {
            if !current_parts.is_empty() {
                units.push(ChunkUnit {
                    heading: current_heading.take(),
                    parts: std::mem::take(&mut current_parts),
                });
            }
            current_heading = Some(text.clone());
            current_parts = vec![text];
        } else {
            current_parts.push(text);
        }
    }

    if !current_parts.is_empty() {
        units.push(ChunkUnit {
            heading: current_heading,
            parts: current_parts,
        });
    }

    if units.is_empty() && !paragraphs.is_empty() {
        let parts: Vec<String> = paragraphs
            .iter()
            .map(|p| p.text.clone())
            .filter(|s| !s.is_empty())
            .collect();
        if !parts.is_empty() {
            return vec![ChunkUnit {
                heading: None,
                parts,
            }];
        }
    }

    units
}

pub fn absorb_heading_only_units(units: Vec<ChunkUnit>) -> Vec<ChunkUnit> {
    let mut result: Vec<ChunkUnit> = Vec::new();
    let mut pending_prefix: Option<String> = None;

    for unit in units {
        let heading_only = unit.heading.is_some()
            && unit.parts.len() == 1
            && Some(unit.parts[0].as_str()) == unit.heading.as_deref();

        if heading_only {
            pending_prefix = unit.heading;
            continue;
        }

        let unit = if let Some(prefix) = pending_prefix.take() {
            let mut new_parts = vec![prefix];
            new_parts.extend(unit.parts);
            ChunkUnit {
                heading: unit.heading,
                parts: new_parts,
            }
        } else {
            unit
        };

        result.push(unit);
    }

    if let Some(prefix) = pending_prefix {
        result.push(ChunkUnit {
            heading: Some(prefix.clone()),
            parts: vec![prefix],
        });
    }

    result
}

pub fn split_unit(unit: &ChunkUnit, max_chars: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current_parts: Vec<String> = Vec::new();
    let mut current_len: usize = 0;

    for part in &unit.parts {
        let part_is_table = is_table_text(part);

        if part_is_table && !current_parts.is_empty() {
            chunks.push(current_parts.join("\n").trim().to_string());
            current_parts.clear();
            current_len = 0;
        }

        if part.len() > max_chars {
            if !current_parts.is_empty() {
                chunks.push(current_parts.join("\n").trim().to_string());
                current_parts.clear();
                current_len = 0;
            }
            chunks.extend(split_large_text(part, max_chars));
            continue;
        }

        if part_is_table {
            chunks.push(part.trim().to_string());
            continue;
        }

        let sep = if current_parts.is_empty() { 0 } else { 1 };
        let candidate = current_len + sep + part.len();

        if candidate <= max_chars {
            current_parts.push(part.clone());
            current_len = candidate;
        } else {
            if !current_parts.is_empty() {
                chunks.push(current_parts.join("\n").trim().to_string());
            }
            current_parts = vec![part.clone()];
            current_len = part.len();
        }
    }

    if !current_parts.is_empty() {
        chunks.push(current_parts.join("\n").trim().to_string());
    }

    chunks.into_iter().filter(|c| !c.is_empty()).collect()
}

pub fn split_large_text(text: &str, max_chars: usize) -> Vec<String> {
    if is_table_text(text) {
        return split_table_text(text, max_chars);
    }

    if text.len() <= max_chars {
        return vec![text.trim().to_string()];
    }

    let sentences = split_sentences(text);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for sentence in sentences {
        let candidate = if current.is_empty() {
            sentence.clone()
        } else {
            format!("{current} {sentence}")
        };

        if candidate.len() <= max_chars {
            current = candidate;
        } else {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
            }
            current = sentence;
            while current.len() > max_chars {
                let split_at = current[..max_chars]
                    .rfind(' ')
                    .unwrap_or(max_chars / 2)
                    .max(max_chars / 2);
                chunks.push(current[..split_at].trim().to_string());
                current = current[split_at..].trim().to_string();
            }
        }
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        chunks.into_iter().filter(|c| !c.is_empty()).collect()
    }
}

// split_sentences — re-exported from super::super::shared above.

pub fn split_table_text(text: &str, max_chars: usize) -> Vec<String> {
    let lines: Vec<String> = text
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        return Vec::new();
    }

    if text.len() <= max_chars || lines.len() == 1 {
        return vec![text.trim().to_string()];
    }

    let header = detect_table_header(&lines);
    let header_prefix = header.map(|line| format!("{line}\n")).unwrap_or_default();
    let start_idx = usize::from(header.is_some());

    let mut chunks: Vec<String> = Vec::new();
    let mut current_rows: Vec<String> = Vec::new();
    let mut current_len = 0usize;

    for row in lines.iter().skip(start_idx) {
        let sep = if current_rows.is_empty() { 0 } else { 1 };
        let candidate_len = header_prefix.len() + current_len + sep + row.len();

        if !current_rows.is_empty() && candidate_len > max_chars {
            chunks.push(
                format!("{}{}", header_prefix, current_rows.join("\n"))
                    .trim()
                    .to_string(),
            );
            current_rows.clear();
            current_len = 0;
        }

        if row.len()
            > max_chars
                .saturating_sub(header_prefix.len())
                .max(max_chars / 2)
        {
            let row_chunks =
                split_plain_text(row, max_chars.saturating_sub(header_prefix.len()).max(200));
            for (idx, row_chunk) in row_chunks.into_iter().enumerate() {
                if idx == 0 && current_rows.is_empty() {
                    current_rows.push(row_chunk.clone());
                    current_len = row_chunk.len();
                } else {
                    if !current_rows.is_empty() {
                        chunks.push(
                            format!("{}{}", header_prefix, current_rows.join("\n"))
                                .trim()
                                .to_string(),
                        );
                        current_rows.clear();
                    }
                    chunks.push(format!("{}{}", header_prefix, row_chunk).trim().to_string());
                    current_len = 0;
                }
            }
            continue;
        }

        current_len += sep + row.len();
        current_rows.push(row.clone());
    }

    if current_rows.is_empty() && chunks.is_empty() {
        return vec![text.trim().to_string()];
    }

    if !current_rows.is_empty() {
        chunks.push(
            format!("{}{}", header_prefix, current_rows.join("\n"))
                .trim()
                .to_string(),
        );
    }

    chunks
        .into_iter()
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

fn split_plain_text(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.trim().to_string()];
    }

    let sentences = split_sentences(text);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for sentence in sentences {
        let candidate = if current.is_empty() {
            sentence.clone()
        } else {
            format!("{current} {sentence}")
        };

        if candidate.len() <= max_chars {
            current = candidate;
        } else {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
            }
            current = sentence;
            while current.len() > max_chars {
                let split_at = current[..max_chars]
                    .rfind(' ')
                    .unwrap_or(max_chars / 2)
                    .max(max_chars / 2);
                chunks.push(current[..split_at].trim().to_string());
                current = current[split_at..].trim().to_string();
            }
        }
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        chunks.into_iter().filter(|c| !c.is_empty()).collect()
    }
}

pub fn classify_chunk(text: &str) -> ContentType {
    if text.is_empty() {
        return ContentType::PlainParagraph;
    }

    if text.starts_with("Table:") || is_table_text(text) {
        return ContentType::Table;
    }

    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();

    if !lines.is_empty()
        && lines
            .iter()
            .all(|l| is_bullet_line(l.trim()) || is_numbered_line(l.trim()))
    {
        return ContentType::BulletNumberedList;
    }

    if text.len() <= 200 && lines.len() <= 4 {
        let joined = lines.join(" ");
        let wc = joined.split_whitespace().count();
        if wc <= 12 && is_heading_style(&joined) {
            return ContentType::HeadingSection;
        }
    }

    if text.len() > 900 {
        ContentType::LongSingleParagraph
    } else if text.len() < 90 {
        ContentType::ShortDisconnectedParagraph
    } else {
        ContentType::PlainParagraph
    }
}

pub fn is_heading_block(joined: &str, lines: &[String]) -> bool {
    let word_count = joined.split_whitespace().count();
    if word_count == 0 || word_count > 10 {
        return false;
    }
    if looks_like_sentence(joined) {
        return false;
    }
    if !joined.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if joined.len() > 150 {
        return false;
    }
    lines.iter().all(|l| is_heading_style(l))
}

pub fn is_heading_style(text: &str) -> bool {
    let t = text.trim();
    let alpha: String = t.chars().filter(|c| c.is_alphabetic()).collect();
    if alpha.is_empty() {
        return false;
    }

    if alpha.len() >= 2 && alpha == alpha.to_uppercase() {
        return true;
    }

    if t.ends_with(':') {
        return true;
    }

    let words: Vec<&str> = t.split_whitespace().collect();

    if words.len() == 1 {
        let first_upper = words[0]
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false);
        return first_upper && words[0].len() >= 4;
    }

    if words.len() <= 8 {
        let title_cased = words
            .iter()
            .all(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(true));
        if title_cased && !looks_like_sentence(t) {
            return true;
        }
    }

    false
}

pub fn is_table_text(text: &str) -> bool {
    let lines: Vec<String> = text
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    is_table_block(&lines)
}

fn is_table_block(lines: &[String]) -> bool {
    if lines.len() < 2 {
        return false;
    }

    let pipe_rows = lines
        .iter()
        .filter(|line| line.matches('|').count() >= 2)
        .count();
    if pipe_rows >= 2 {
        return true;
    }

    let candidate_rows: Vec<(usize, bool)> = lines
        .iter()
        .filter_map(|line| {
            let count = table_token_count(line);
            if count >= 3 && line.len() <= 160 {
                Some((count, contains_numericish_token(line)))
            } else {
                None
            }
        })
        .collect();

    if candidate_rows.len() < 2 {
        return false;
    }

    let min_cols = candidate_rows
        .iter()
        .map(|(count, _)| *count)
        .min()
        .unwrap_or(0);
    let max_cols = candidate_rows
        .iter()
        .map(|(count, _)| *count)
        .max()
        .unwrap_or(0);
    let numeric_rows = candidate_rows
        .iter()
        .filter(|(_, numeric)| *numeric)
        .count();
    let short_non_sentence_rows = lines
        .iter()
        .filter(|line| !looks_like_sentence(line))
        .count();

    max_cols.saturating_sub(min_cols) <= 2
        && numeric_rows >= 2
        && short_non_sentence_rows >= lines.len().saturating_sub(1)
}

fn table_token_count(line: &str) -> usize {
    if line.contains('|') {
        return line
            .split('|')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .count();
    }

    line.split_whitespace()
        .filter(|token| !token.is_empty())
        .count()
}

fn contains_numericish_token(line: &str) -> bool {
    line.split_whitespace().any(is_numericish_token)
}

fn is_numericish_token(token: &str) -> bool {
    let stripped =
        token.trim_matches(|c: char| matches!(c, ',' | '$' | '%' | '(' | ')' | '[' | ']'));

    !stripped.is_empty()
        && stripped.chars().any(|ch| ch.is_ascii_digit())
        && stripped
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '/' | ':'))
}

fn detect_table_header(lines: &[String]) -> Option<&str> {
    if lines.len() < 3 {
        return None;
    }

    let first_numeric = contains_numericish_token(&lines[0]);
    let following_numeric_rows = lines
        .iter()
        .skip(1)
        .filter(|line| contains_numericish_token(line))
        .count();

    if !first_numeric && following_numeric_rows >= 2 {
        Some(lines[0].as_str())
    } else {
        None
    }
}

pub fn normalize_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn is_noise_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with("copyright ") || is_standalone_number(line)
}

pub fn is_standalone_number(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && t.len() <= 3 && t.chars().all(|c| c.is_ascii_digit())
}

pub fn is_toc_heading(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower == "table of content" || lower == "table of contents"
}

pub fn looks_like_toc_entry(line: &str) -> bool {
    let t = line.trim();
    let mut parts = t.rsplitn(2, ' ');
    let last = parts.next().unwrap_or_default();
    let rest = parts.next().unwrap_or_default();
    !rest.is_empty()
        && last.chars().all(|c| c.is_ascii_digit())
        && rest.chars().any(|c| c.is_ascii_alphabetic())
        && rest.split_whitespace().count() <= 8
}

pub fn is_bullet_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("\u{2022} ")
        || t.starts_with('\u{2022}')
        || t.starts_with('\u{25E6}')
        || t.starts_with('\u{25AA}')
        || t.starts_with('\u{25B8}')
        || t.starts_with('\u{25B9}')
        || t.starts_with('\u{25BA}')
        || t.starts_with('\u{2023}')
        || t.starts_with('\u{2043}')
}

pub fn is_numbered_line(line: &str) -> bool {
    let t = line.trim();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 3 {
        return false;
    }
    let rest = &t[digits.len()..];
    matches!(rest.chars().next(), Some('.') | Some(')')) && rest.len() > 2
}

pub fn looks_like_sentence(line: &str) -> bool {
    let words = line.split_whitespace().count();
    words >= 8 || line.ends_with('.') || line.ends_with('!') || line.ends_with('?')
}

pub fn merge_bullet_lines(lines: &[String]) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();

    for line in lines {
        if is_bullet_line(line) || is_numbered_line(line) {
            merged.push(line.clone());
        } else if let Some(prev) = merged.last_mut() {
            if is_bullet_line(prev) || is_numbered_line(prev) {
                prev.push(' ');
                prev.push_str(line);
            } else {
                merged.push(line.clone());
            }
        } else {
            merged.push(line.clone());
        }
    }

    merged
}
