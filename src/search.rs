use crate::rendered::{ImageSlot, LineContent, RenderLine, Segment};
use crate::selection::{SelectionPoint, SelectionRange};
use crate::width::{char_width, width_chars};

pub fn find_matches(lines: &[RenderLine], query: &str) -> Vec<SelectionRange> {
    let query = fold_case(query);
    if query.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for (row, text) in rendered_rows(lines).into_iter().enumerate() {
        matches.extend(find_line_matches(row, &text, &query));
    }
    matches
}

pub fn first_at_or_after(matches: &[SelectionRange], row: usize) -> Option<usize> {
    matches
        .iter()
        .position(|range| range.start.row >= row)
        .or_else(|| (!matches.is_empty()).then_some(0))
}

pub fn next_index(matches: &[SelectionRange], current: Option<usize>) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(current.map_or(0, |index| (index + 1) % matches.len()))
}

pub fn previous_index(matches: &[SelectionRange], current: Option<usize>) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(match current {
        Some(0) | None => matches.len() - 1,
        Some(index) => index.saturating_sub(1).min(matches.len() - 1),
    })
}

fn rendered_rows(lines: &[RenderLine]) -> Vec<String> {
    let mut rows = Vec::new();
    for line in lines {
        match &line.content {
            LineContent::Text(segments) => rows.push(segments_text(segments)),
            LineContent::Image(ImageSlot { alt, .. }) => {
                rows.push(format!("[image: {alt}]"));
                for _ in 1..line.height() {
                    rows.push(String::new());
                }
            }
        }
    }
    rows
}

fn segments_text(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect()
}

#[derive(Debug, Clone)]
struct CharCell {
    folded: String,
    start_col: usize,
    end_col: usize,
}

fn find_line_matches(row: usize, text: &str, query: &str) -> Vec<SelectionRange> {
    let chars = char_cells(text);
    let mut matches = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        let mut folded = String::new();
        let mut end = start;
        let mut matched = false;
        while end < chars.len() && folded.len() <= query.len() {
            folded.push_str(&chars[end].folded);
            end += 1;
            if folded == query {
                matches.push(SelectionRange {
                    start: SelectionPoint::new(row, chars[start].start_col),
                    end: SelectionPoint::new(row, chars[end - 1].end_col),
                });
                start = end;
                matched = true;
                break;
            }
            if !query.starts_with(&folded) {
                break;
            }
        }
        if !matched {
            start += 1;
        }
    }

    matches
}

fn char_cells(text: &str) -> Vec<CharCell> {
    let mut cells: Vec<CharCell> = Vec::new();
    let mut col = 0usize;
    let mut cluster_start = 0usize;
    let mut regional_run = 0usize;
    for (ch, width) in width_chars(text) {
        let scalar_width = char_width(ch).unwrap_or(0);
        let regional_indicator = matches!(ch as u32, 0x1f1e6..=0x1f1ff);
        let continues_cluster = !cells.is_empty()
            && (width == 0 || scalar_width == 0 || (regional_indicator && regional_run % 2 == 1));

        if !continues_cluster {
            cluster_start = cells.len();
        }
        let start_col = if continues_cluster {
            cells[cluster_start].start_col
        } else {
            col
        };
        col = col.saturating_add(width);
        if continues_cluster {
            for cell in &mut cells[cluster_start..] {
                cell.end_col = col;
            }
        }

        cells.push(CharCell {
            folded: fold_char(ch),
            start_col,
            end_col: col,
        });
        regional_run = if regional_indicator {
            regional_run.saturating_add(1)
        } else {
            0
        };
    }
    cells
}

fn fold_case(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

fn fold_char(ch: char) -> String {
    ch.to_lowercase().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendered::plain_line;

    #[test]
    fn finds_case_insensitive_matches() {
        let lines = vec![plain_line("Alpha alpha ALPHA")];
        let matches = find_matches(&lines, "alpha");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].start, SelectionPoint::new(0, 0));
        assert_eq!(matches[1].start, SelectionPoint::new(0, 6));
        assert_eq!(matches[2].start, SelectionPoint::new(0, 12));
    }

    #[test]
    fn finds_first_match_at_or_after_row_with_wrap() {
        let matches = vec![
            SelectionRange {
                start: SelectionPoint::new(2, 0),
                end: SelectionPoint::new(2, 5),
            },
            SelectionRange {
                start: SelectionPoint::new(8, 0),
                end: SelectionPoint::new(8, 5),
            },
        ];
        assert_eq!(first_at_or_after(&matches, 5), Some(1));
        assert_eq!(first_at_or_after(&matches, 9), Some(0));
    }

    #[test]
    fn navigates_match_indices() {
        let matches = vec![
            SelectionRange {
                start: SelectionPoint::new(0, 0),
                end: SelectionPoint::new(0, 1),
            },
            SelectionRange {
                start: SelectionPoint::new(1, 0),
                end: SelectionPoint::new(1, 1),
            },
        ];
        assert_eq!(next_index(&matches, None), Some(0));
        assert_eq!(next_index(&matches, Some(0)), Some(1));
        assert_eq!(next_index(&matches, Some(1)), Some(0));
        assert_eq!(previous_index(&matches, Some(0)), Some(1));
        assert_eq!(previous_index(&matches, Some(1)), Some(0));
    }

    #[test]
    fn reports_terminal_cell_columns() {
        let lines = vec![plain_line("a界B")];
        let matches = find_matches(&lines, "界b");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, SelectionPoint::new(0, 1));
        assert_eq!(matches[0].end, SelectionPoint::new(0, 4));
    }

    #[test]
    fn expands_sequence_character_matches_to_the_displayed_cluster() {
        let lines = vec![plain_line("👩‍💻 ❤️‍🔥 🇵🇱🇺")];

        let laptop = find_matches(&lines, "💻");
        assert_eq!(laptop[0].start, SelectionPoint::new(0, 0));
        assert_eq!(laptop[0].end, SelectionPoint::new(0, 2));

        let fire = find_matches(&lines, "🔥");
        assert_eq!(fire[0].start, SelectionPoint::new(0, 3));
        assert_eq!(fire[0].end, SelectionPoint::new(0, 5));

        let second_flag = find_matches(&lines, "🇺");
        assert_eq!(second_flag[0].start, SelectionPoint::new(0, 8));
        assert_eq!(second_flag[0].end, SelectionPoint::new(0, 9));
    }
}
