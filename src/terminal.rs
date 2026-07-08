use crate::image::{
    base64_encode, detect_image_mode, iterm2_image_sequence, tmux_passthrough, ImageMode,
};
use crate::rendered::{ImageSlot, LineContent, RenderLine, Segment, Style};
use crate::selection::SelectionRange;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};
use std::env;
use std::fs;
use std::io::{self, Stdout, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub struct Terminal {
    stdout: Stdout,
    image_mode: ImageMode,
}

impl Terminal {
    pub fn enter() -> io::Result<Self> {
        let mut stdout = io::stdout();
        terminal::enable_raw_mode()?;
        if let Err(err) = execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            Hide,
            Clear(ClearType::All)
        ) {
            let _ = terminal::disable_raw_mode();
            return Err(err);
        }

        Ok(Self {
            stdout,
            image_mode: detect_image_mode(),
        })
    }

    pub fn viewport_size(&self) -> io::Result<(u16, u16)> {
        let (cols, rows) = terminal::size()?;
        Ok((cols.max(1), rows.saturating_sub(1).max(1)))
    }

    pub fn draw(
        &mut self,
        lines: &[RenderLine],
        scroll_offset: usize,
        status: &str,
        selection: Option<SelectionRange>,
        search_highlights: &[SelectionRange],
    ) -> io::Result<()> {
        let (cols, rows) = terminal::size()?;
        let cols = cols.max(1);
        let viewport_rows = rows.saturating_sub(1);

        queue!(self.stdout, MoveTo(0, 0), Clear(ClearType::All))?;
        self.draw_body(
            lines,
            scroll_offset,
            cols,
            viewport_rows,
            selection,
            search_highlights,
        )?;
        self.draw_status(status, cols, rows.saturating_sub(1))?;
        self.stdout.flush()
    }

    pub fn copy_to_clipboard(&mut self, text: &str) -> io::Result<()> {
        let sequence = clipboard_sequence(text);
        write!(self.stdout, "{sequence}")?;
        self.stdout.flush()
    }

    fn draw_body(
        &mut self,
        lines: &[RenderLine],
        scroll_offset: usize,
        cols: u16,
        viewport_rows: u16,
        selection: Option<SelectionRange>,
        search_highlights: &[SelectionRange],
    ) -> io::Result<()> {
        let mut skip = scroll_offset;
        let mut y = 0u16;

        for line in lines {
            let line_height = line.height();
            if skip >= line_height {
                skip -= line_height;
                continue;
            }

            if skip > 0 {
                y = y.saturating_add((line_height - skip) as u16);
                skip = 0;
                if y >= viewport_rows {
                    break;
                }
                continue;
            }

            if y >= viewport_rows {
                break;
            }

            match &line.content {
                LineContent::Text(segments) => {
                    queue!(self.stdout, MoveTo(0, y))?;
                    let absolute_row = scroll_offset + y as usize;
                    let selected_cols = selection
                        .and_then(|range| range.columns_for_row(absolute_row, cols as usize));
                    let search_cols = search_highlights
                        .iter()
                        .filter_map(|range| range.columns_for_row(absolute_row, cols as usize))
                        .collect::<Vec<_>>();
                    write_segments_with_highlights(
                        &mut self.stdout,
                        segments,
                        cols as usize,
                        selected_cols,
                        &search_cols,
                    )?;
                    y += 1;
                }
                LineContent::Image(slot) => {
                    self.draw_image(slot, y, cols, viewport_rows)?;
                    y = y.saturating_add(slot.height_cells.max(1));
                }
            }
        }

        Ok(())
    }

    fn draw_image(
        &mut self,
        slot: &ImageSlot,
        y: u16,
        cols: u16,
        viewport_rows: u16,
    ) -> io::Result<()> {
        if y + slot.height_cells.max(1) > viewport_rows {
            queue!(self.stdout, MoveTo(0, y))?;
            return write_fallback_image(
                &mut self.stdout,
                slot,
                cols as usize,
                "not enough visible rows",
            );
        }

        match &self.image_mode {
            ImageMode::Direct | ImageMode::TmuxPassthrough => {
                let data = match fs::read(&slot.path) {
                    Ok(data) => data,
                    Err(err) => {
                        queue!(self.stdout, MoveTo(0, y))?;
                        return write_fallback_image(
                            &mut self.stdout,
                            slot,
                            cols as usize,
                            &format!("failed to read: {err}"),
                        );
                    }
                };
                let name = slot
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("image");
                let sequence =
                    iterm2_image_sequence(&data, name, slot.width_cells, slot.height_cells);
                let sequence = if matches!(self.image_mode, ImageMode::TmuxPassthrough) {
                    tmux_passthrough(&sequence)
                } else {
                    sequence
                };
                queue!(self.stdout, MoveTo(0, y))?;
                write!(self.stdout, "{sequence}")?;
                queue!(
                    self.stdout,
                    MoveTo(0, y.saturating_add(slot.height_cells.max(1)))
                )?;
                Ok(())
            }
            ImageMode::Disabled(reason) => {
                queue!(self.stdout, MoveTo(0, y))?;
                write_fallback_image(&mut self.stdout, slot, cols as usize, reason)
            }
        }
    }

    fn draw_status(&mut self, status: &str, cols: u16, y: u16) -> io::Result<()> {
        let mut text = fit_to_width(status, cols as usize);
        let width = UnicodeWidthStr::width(text.as_str());
        if width < cols as usize {
            text.push_str(&" ".repeat(cols as usize - width));
        }
        queue!(self.stdout, MoveTo(0, y))?;
        write_segments(
            &mut self.stdout,
            &[Segment::new(text, Style::status())],
            cols as usize,
        )
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(
            self.stdout,
            Show,
            DisableMouseCapture,
            LeaveAlternateScreen,
            Clear(ClearType::All)
        );
        let _ = terminal::disable_raw_mode();
    }
}

fn write_fallback_image(
    out: &mut impl Write,
    slot: &ImageSlot,
    max_width: usize,
    reason: &str,
) -> io::Result<()> {
    let mut style = Style::plain();
    style.dim = true;
    let text = format!(
        "[image: {}] {} ({})",
        if slot.alt.is_empty() {
            "image"
        } else {
            &slot.alt
        },
        slot.path.display(),
        reason
    );
    write_segments(out, &[Segment::new(text, style)], max_width)
}

pub fn write_segments(
    out: &mut impl Write,
    segments: &[Segment],
    max_width: usize,
) -> io::Result<()> {
    write_segments_with_highlights(out, segments, max_width, None, &[])
}

fn write_segments_with_highlights(
    out: &mut impl Write,
    segments: &[Segment],
    max_width: usize,
    selection: Option<(usize, usize)>,
    search_highlights: &[(usize, usize)],
) -> io::Result<()> {
    let mut col = 0usize;
    let mut pending: Option<Segment> = None;

    'segments: for segment in segments {
        if col >= max_width {
            break;
        }

        let text = visible_safe(&segment.text);
        let mut last_selected = false;
        for ch in text.chars() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if char_width > 0 && col + char_width > max_width {
                break 'segments;
            }

            let selected = selection.is_some_and(|(start, end)| {
                if char_width == 0 {
                    last_selected
                } else {
                    col < end && col + char_width > start
                }
            });
            let mut style = segment.style.clone();
            if column_in_ranges(col, char_width, search_highlights) {
                let highlight = Style::search_highlight();
                style.fg = highlight.fg;
                style.bg = highlight.bg;
                style.bold = true;
            }
            if selected {
                style.reverse = true;
            }
            queue_output_segment(
                out,
                &mut pending,
                Segment {
                    text: ch.to_string(),
                    style,
                    link: segment.link.clone(),
                },
            )?;

            if char_width > 0 {
                col += char_width;
                last_selected = selected;
            }
        }
    }

    flush_pending_segment(out, &mut pending)?;
    write!(out, "\x1b[0m")
}

fn column_in_ranges(col: usize, width: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|(start, end)| {
        if width == 0 {
            *start <= col && col < *end
        } else {
            col < *end && col + width > *start
        }
    })
}

fn queue_output_segment(
    out: &mut impl Write,
    pending: &mut Option<Segment>,
    segment: Segment,
) -> io::Result<()> {
    if segment.text.is_empty() {
        return Ok(());
    }
    if let Some(current) = pending {
        if current.style == segment.style && current.link == segment.link {
            current.text.push_str(&segment.text);
            return Ok(());
        }
    }

    flush_pending_segment(out, pending)?;
    let _ = pending.replace(segment);
    Ok(())
}

fn flush_pending_segment(out: &mut impl Write, pending: &mut Option<Segment>) -> io::Result<()> {
    let Some(segment) = pending.take() else {
        return Ok(());
    };
    if let Some(link) = &segment.link {
        write!(out, "{}", osc8_start(link))?;
    }
    write!(out, "{}", segment.style.sgr())?;
    write!(out, "{}", segment.text)?;
    write!(out, "\x1b[0m")?;
    if segment.link.is_some() {
        write!(out, "{}", osc8_end())?;
    }
    Ok(())
}

pub fn osc8_start(uri: &str) -> String {
    format!("\x1b]8;;{}\x1b\\", osc_safe(uri))
}

pub fn osc8_end() -> &'static str {
    "\x1b]8;;\x1b\\"
}

pub fn fit_to_width(text: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + char_width > max_width {
            break;
        }
        out.push(ch);
        width += char_width;
    }
    out
}

fn visible_safe(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch == '\t' || !ch.is_control() {
                ch
            } else {
                ' '
            }
        })
        .collect()
}

fn osc_safe(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, '\x07' | '\x1b' | '\u{9b}'))
        .collect()
}

fn clipboard_sequence(text: &str) -> String {
    clipboard_sequence_from_env(text, |key| env::var(key).ok())
}

fn clipboard_sequence_from_env<F>(text: &str, env_get: F) -> String
where
    F: Fn(&str) -> Option<String>,
{
    let encoded = base64_encode(text.as_bytes());
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    if inside_tmux_or_screen(env_get) {
        format!("{}{}", sequence, tmux_passthrough(&sequence))
    } else {
        sequence
    }
}

fn inside_tmux_or_screen<F>(env_get: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    let term_program = env_get("TERM_PROGRAM").unwrap_or_default();
    let term = env_get("TERM").unwrap_or_default();
    env_get("TMUX").is_some()
        || term_program == "tmux"
        || term.starts_with("tmux")
        || term.starts_with("screen")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendered::{Style, TermColor};

    #[test]
    fn osc8_filters_control_terminators() {
        assert_eq!(
            osc8_start("https://example.test/\x1b\x07"),
            "\x1b]8;;https://example.test/\x1b\\"
        );
        assert_eq!(osc8_end(), "\x1b]8;;\x1b\\");
    }

    #[test]
    fn fits_text_to_terminal_cells() {
        assert_eq!(fit_to_width("abcdef", 3), "abc");
        assert_eq!(fit_to_width("a界b", 3), "a界");
    }

    #[test]
    fn writes_styled_segments_with_hyperlink() {
        let mut out = Vec::new();
        let style = Style {
            fg: Some(TermColor::Rgb(1, 2, 3)),
            underline: true,
            ..Style::plain()
        };
        write_segments(
            &mut out,
            &[Segment::with_link("Rust", style, "https://rust-lang.org")],
            80,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\x1b]8;;https://rust-lang.org\x1b\\"));
        assert!(text.contains("\x1b[0;4;38;2;1;2;3mRust\x1b[0m"));
        assert!(text.contains(osc8_end()));
    }

    #[test]
    fn writes_selected_text_with_reverse_video() {
        let mut out = Vec::new();
        write_segments_with_highlights(
            &mut out,
            &[Segment::new("abcdef", Style::plain())],
            80,
            Some((1, 4)),
            &[],
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\x1b[0ma\x1b[0m"));
        assert!(text.contains("\x1b[0;7mbcd\x1b[0m"));
        assert!(text.contains("\x1b[0mef\x1b[0m"));
    }

    #[test]
    fn writes_search_highlights_with_background() {
        let mut out = Vec::new();
        write_segments_with_highlights(
            &mut out,
            &[Segment::new("abcdef", Style::plain())],
            80,
            None,
            &[(2, 5)],
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\x1b[0;1;38;2;36;41;47;48;2;255;235;132mcde\x1b[0m"));
    }

    #[test]
    fn stops_when_next_terminal_cell_cannot_fit() {
        let mut out = Vec::new();
        write_segments(
            &mut out,
            &[
                Segment::new("a界", Style::plain()),
                Segment::new("b", Style::plain()),
            ],
            2,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\x1b[0ma\x1b[0m"));
        assert!(!text.contains('b'));
    }

    #[test]
    fn builds_osc52_clipboard_sequences() {
        assert_eq!(
            clipboard_sequence_from_env("copy", |_| None),
            "\x1b]52;c;Y29weQ==\x07"
        );

        let wrapped = clipboard_sequence_from_env("copy", |key| {
            (key == "TMUX").then_some("/tmp/tmux-1000/default,1,0".to_string())
        });
        assert!(wrapped.starts_with("\x1b]52;c;Y29weQ==\x07"));
        assert!(wrapped.contains("\x1bPtmux;"));
        assert!(wrapped.contains("\x1b\x1b]52;c;Y29weQ==\x07"));
        assert!(wrapped.ends_with("\x1b\\"));
    }
}
