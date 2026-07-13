use crate::rendered::{ImageSlot, LineContent, RenderLine, Segment};
use crate::width::width_chars;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SelectionPoint {
    pub row: usize,
    pub col: usize,
}

impl SelectionPoint {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRange {
    pub start: SelectionPoint,
    pub end: SelectionPoint,
}

impl SelectionRange {
    pub fn columns_for_row(&self, row: usize, max_width: usize) -> Option<(usize, usize)> {
        if row < self.start.row || row > self.end.row {
            return None;
        }

        let (start, end) = if self.start.row == self.end.row {
            (self.start.col, self.end.col)
        } else if row == self.start.row {
            (self.start.col, max_width)
        } else if row == self.end.row {
            (0, self.end.col)
        } else {
            (0, max_width)
        };

        (start < end).then_some((start.min(max_width), end.min(max_width)))
    }
}

#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    anchor: Option<SelectionPoint>,
    focus: Option<SelectionPoint>,
    active: bool,
    dragged: bool,
}

impl SelectionState {
    pub fn begin(&mut self, point: SelectionPoint) {
        self.anchor = Some(point);
        self.focus = Some(point);
        self.active = true;
        self.dragged = false;
    }

    pub fn update(&mut self, point: SelectionPoint) {
        if self.active {
            if self.anchor.is_some_and(|anchor| anchor != point) {
                self.dragged = true;
            }
            self.focus = Some(point);
        }
    }

    pub fn finish(&mut self, point: SelectionPoint) {
        self.update(point);
        self.active = false;
    }

    pub fn clear(&mut self) {
        self.anchor = None;
        self.focus = None;
        self.active = false;
        self.dragged = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn range(&self) -> Option<SelectionRange> {
        if !self.dragged {
            return None;
        }

        let anchor = self.anchor?;
        let focus = self.focus?;
        let (start, end) = if anchor <= focus {
            (
                anchor,
                SelectionPoint::new(focus.row, focus.col.saturating_add(1)),
            )
        } else {
            (
                focus,
                SelectionPoint::new(anchor.row, anchor.col.saturating_add(1)),
            )
        };

        (start < end).then_some(SelectionRange { start, end })
    }
}

pub fn selected_text(
    lines: &[RenderLine],
    range: SelectionRange,
    max_width: usize,
) -> Option<String> {
    let mut rows = Vec::new();
    let mut row = 0usize;
    for line in lines {
        if row > range.end.row {
            break;
        }
        match &line.content {
            LineContent::Text(segments) => {
                push_selected_row(&mut rows, &segments_text(segments), row, range, max_width);
                row += 1;
            }
            LineContent::Image(ImageSlot { alt, .. }) => {
                let height = line.height();
                for image_row in 0..height {
                    let text = if image_row == 0 {
                        format!("[image: {alt}]")
                    } else {
                        String::new()
                    };
                    push_selected_row(&mut rows, &text, row + image_row, range, max_width);
                }
                row += height;
            }
        }
    }

    let text = rows.join("\n");
    (!text.is_empty()).then_some(text)
}

fn push_selected_row(
    rows: &mut Vec<String>,
    text: &str,
    row: usize,
    range: SelectionRange,
    max_width: usize,
) {
    let Some((start_col, end_col)) = range.columns_for_row(row, max_width) else {
        return;
    };
    rows.push(slice_cells(text, start_col, end_col).trim_end().to_string());
}

fn segments_text(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect()
}

fn slice_cells(text: &str, start_col: usize, end_col: usize) -> String {
    if start_col >= end_col {
        return String::new();
    }

    let mut out = String::new();
    let mut col = 0usize;
    let mut last_selected = false;
    for (ch, width) in width_chars(text) {
        let selected = if width == 0 {
            last_selected
        } else {
            col < end_col && col + width > start_col
        };
        if selected {
            out.push(ch);
        }
        if width > 0 {
            col += width;
            last_selected = selected;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendered::{plain_line, RenderLine, Segment, Style};

    #[test]
    fn normalizes_reverse_drag() {
        let mut selection = SelectionState::default();
        selection.begin(SelectionPoint::new(3, 8));
        selection.update(SelectionPoint::new(1, 2));
        let range = selection.range().unwrap();
        assert_eq!(range.start, SelectionPoint::new(1, 2));
        assert_eq!(range.end, SelectionPoint::new(3, 9));
    }

    #[test]
    fn ignores_single_click_without_drag() {
        let mut selection = SelectionState::default();
        selection.begin(SelectionPoint::new(0, 0));
        selection.finish(SelectionPoint::new(0, 0));
        assert_eq!(selection.range(), None);
    }

    #[test]
    fn extracts_selected_text_across_rows() {
        let lines = vec![plain_line("alpha"), plain_line("beta"), plain_line("gamma")];
        let range = SelectionRange {
            start: SelectionPoint::new(0, 1),
            end: SelectionPoint::new(2, 3),
        };
        assert_eq!(selected_text(&lines, range, 80).unwrap(), "lpha\nbeta\ngam");
    }

    #[test]
    fn slices_by_terminal_cell_width() {
        let lines = vec![RenderLine::text(vec![Segment::new("a界b", Style::plain())])];
        let range = SelectionRange {
            start: SelectionPoint::new(0, 1),
            end: SelectionPoint::new(0, 3),
        };
        assert_eq!(selected_text(&lines, range, 80).unwrap(), "界");
    }
}
