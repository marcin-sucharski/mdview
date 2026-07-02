use crate::image::{detect_image_mode, iterm2_image_sequence, tmux_passthrough, ImageMode};
use crate::rendered::{ImageSlot, LineContent, RenderLine, Segment, Style};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};
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
    ) -> io::Result<()> {
        let (cols, rows) = terminal::size()?;
        let cols = cols.max(1);
        let viewport_rows = rows.saturating_sub(1);

        queue!(self.stdout, MoveTo(0, 0), Clear(ClearType::All))?;
        self.draw_body(lines, scroll_offset, cols, viewport_rows)?;
        self.draw_status(status, cols, rows.saturating_sub(1))?;
        self.stdout.flush()
    }

    fn draw_body(
        &mut self,
        lines: &[RenderLine],
        scroll_offset: usize,
        cols: u16,
        viewport_rows: u16,
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
                    write_segments(&mut self.stdout, segments, cols as usize)?;
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
    let mut col = 0usize;
    for segment in segments {
        if col >= max_width {
            break;
        }
        let text = fit_to_width(&visible_safe(&segment.text), max_width - col);
        if text.is_empty() {
            continue;
        }
        if let Some(link) = &segment.link {
            write!(out, "{}", osc8_start(link))?;
        }
        write!(out, "{}", segment.style.sgr())?;
        write!(out, "{text}")?;
        write!(out, "\x1b[0m")?;
        if segment.link.is_some() {
            write!(out, "{}", osc8_end())?;
        }
        col += UnicodeWidthStr::width(text.as_str());
    }
    write!(out, "\x1b[0m")
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
}
