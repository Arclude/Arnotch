//! Small, display-ready diff excerpts of file edits.

use std::fs;
use std::time::Duration;

use similar::{ChangeTag, TextDiff};

use crate::event::{Diff, DiffLine, LineKind};

/// The island has room for this many lines.
const MAX_LINES: usize = 14;
const MAX_LINE_CHARS: usize = 160;
/// Bigger files are not read: hooks run on every tool call and must stay fast.
const MAX_FILE_BYTES: u64 = 512 * 1024;
const DIFF_TIMEOUT: Duration = Duration::from_millis(30);

/// Diff of `old` → `new`, where both start at line `first_line` of `file` (0 when unknown).
pub fn excerpt(file: &str, old: &str, new: &str, first_line: u32) -> Diff {
    let diff = TextDiff::configure()
        .timeout(DIFF_TIMEOUT)
        .diff_lines(old, new);

    let mut lines = Vec::new();
    let mut truncated = false;

    'groups: for (i, group) in diff.grouped_ops(2).iter().enumerate() {
        if i > 0 {
            lines.push(DiffLine {
                kind: LineKind::Ctx,
                n: 0,
                text: "⋯".into(),
            });
        }
        for op in group {
            for change in diff.iter_changes(op) {
                if lines.len() >= MAX_LINES {
                    truncated = true;
                    break 'groups;
                }
                let (kind, index) = match change.tag() {
                    ChangeTag::Insert => (LineKind::Add, change.new_index()),
                    ChangeTag::Delete => (LineKind::Del, change.old_index()),
                    ChangeTag::Equal => (LineKind::Ctx, change.new_index()),
                };
                let n = match (first_line, index) {
                    (0, _) | (_, None) => 0,
                    (first, Some(index)) => first + index as u32,
                };
                lines.push(DiffLine {
                    kind,
                    n,
                    text: display_line(&change.to_string_lossy()),
                });
            }
        }
    }

    Diff {
        file: file.to_owned(),
        lines,
        truncated,
    }
}

/// Reads a file unless it is too big to diff cheaply.
pub fn read_small(path: &str) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

/// 1-based line where `needle` first appears in `text`, 0 if it doesn't.
pub fn line_of(text: &str, needle: &str) -> u32 {
    text.find(needle)
        .map_or(0, |at| text[..at].matches('\n').count() as u32 + 1)
}

fn display_line(line: &str) -> String {
    let line = line.trim_end_matches(['\n', '\r']).replace('\t', "    ");
    match line.char_indices().nth(MAX_LINE_CHARS) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(diff: &Diff) -> Vec<String> {
        diff.lines
            .iter()
            .map(|l| {
                let sign = match l.kind {
                    LineKind::Add => '+',
                    LineKind::Del => '-',
                    LineKind::Ctx => ' ',
                };
                format!("{}{sign}{}", l.n, l.text)
            })
            .collect()
    }

    #[test]
    fn numbers_lines_from_the_snippet_start() {
        let old = "const TAX = 0.2;\nreturn sum;\n";
        let new = "const TAX = 0.18;\nreturn sum;\n";
        let diff = excerpt("invoice.ts", old, new, 12);
        assert_eq!(
            render(&diff),
            [
                "12-const TAX = 0.2;",
                "12+const TAX = 0.18;",
                "13 return sum;"
            ]
        );
        assert!(!diff.truncated);
    }

    #[test]
    fn caps_the_number_of_lines() {
        let new: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let diff = excerpt("big.rs", "", &new, 1);
        assert_eq!(diff.lines.len(), MAX_LINES);
        assert!(diff.truncated);
    }

    #[test]
    fn finds_line_numbers() {
        assert_eq!(line_of("a\nb\nc\n", "c"), 3);
        assert_eq!(line_of("a\nb\n", "zzz"), 0);
    }
}
