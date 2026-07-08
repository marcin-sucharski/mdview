use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermColor {
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Option<TermColor>,
    pub bg: Option<TermColor>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

impl Style {
    pub fn plain() -> Self {
        Self::default()
    }

    pub fn heading(level: u8) -> Self {
        let fg = match level {
            1 => TermColor::Rgb(0, 90, 138),
            2 => TermColor::Rgb(36, 105, 71),
            3 => TermColor::Rgb(122, 77, 0),
            _ => TermColor::Rgb(91, 75, 138),
        };
        Self {
            fg: Some(fg),
            bold: true,
            ..Self::default()
        }
    }

    pub fn heading_banner() -> Self {
        Self {
            fg: Some(TermColor::Rgb(255, 255, 255)),
            bg: Some(TermColor::Rgb(0, 90, 138)),
            bold: true,
            ..Self::default()
        }
    }

    pub fn heading_panel(level: u8) -> Self {
        let (fg, bg) = match level {
            2 => (TermColor::Rgb(25, 88, 61), TermColor::Rgb(232, 245, 237)),
            3 => (TermColor::Rgb(122, 77, 0), TermColor::Rgb(255, 248, 219)),
            _ => (TermColor::Rgb(91, 75, 138), TermColor::Rgb(244, 240, 255)),
        };
        Self {
            fg: Some(fg),
            bg: Some(bg),
            bold: true,
            ..Self::default()
        }
    }

    pub fn heading_rule(level: u8) -> Self {
        let fg = match level {
            1 => TermColor::Rgb(0, 90, 138),
            2 => TermColor::Rgb(36, 105, 71),
            3 => TermColor::Rgb(191, 135, 0),
            _ => TermColor::Rgb(91, 75, 138),
        };
        Self {
            fg: Some(fg),
            dim: level > 2,
            ..Self::default()
        }
    }

    pub fn code() -> Self {
        Self {
            fg: Some(TermColor::Rgb(96, 56, 19)),
            bg: Some(TermColor::Rgb(246, 248, 250)),
            ..Self::default()
        }
    }

    pub fn code_keyword() -> Self {
        Self {
            fg: Some(TermColor::Rgb(91, 75, 138)),
            bg: Some(TermColor::Rgb(246, 248, 250)),
            bold: true,
            ..Self::default()
        }
    }

    pub fn code_key() -> Self {
        Self {
            fg: Some(TermColor::Rgb(5, 112, 133)),
            bg: Some(TermColor::Rgb(246, 248, 250)),
            bold: true,
            ..Self::default()
        }
    }

    pub fn code_string() -> Self {
        Self {
            fg: Some(TermColor::Rgb(17, 99, 41)),
            bg: Some(TermColor::Rgb(246, 248, 250)),
            ..Self::default()
        }
    }

    pub fn code_number() -> Self {
        Self {
            fg: Some(TermColor::Rgb(9, 105, 218)),
            bg: Some(TermColor::Rgb(246, 248, 250)),
            ..Self::default()
        }
    }

    pub fn code_literal() -> Self {
        Self {
            fg: Some(TermColor::Rgb(149, 56, 0)),
            bg: Some(TermColor::Rgb(246, 248, 250)),
            bold: true,
            ..Self::default()
        }
    }

    pub fn code_punctuation() -> Self {
        Self {
            fg: Some(TermColor::Rgb(87, 96, 106)),
            bg: Some(TermColor::Rgb(246, 248, 250)),
            ..Self::default()
        }
    }

    pub fn code_comment() -> Self {
        Self {
            fg: Some(TermColor::Rgb(106, 115, 125)),
            bg: Some(TermColor::Rgb(246, 248, 250)),
            dim: true,
            ..Self::default()
        }
    }

    pub fn quote() -> Self {
        Self {
            fg: Some(TermColor::Rgb(88, 96, 105)),
            italic: true,
            ..Self::default()
        }
    }

    pub fn link() -> Self {
        Self {
            fg: Some(TermColor::Rgb(9, 105, 218)),
            underline: true,
            ..Self::default()
        }
    }

    pub fn table_border() -> Self {
        Self {
            fg: Some(TermColor::Rgb(106, 115, 125)),
            dim: true,
            ..Self::default()
        }
    }

    pub fn table_header() -> Self {
        Self {
            fg: Some(TermColor::Rgb(36, 41, 47)),
            bg: Some(TermColor::Rgb(232, 245, 237)),
            bold: true,
            ..Self::default()
        }
    }

    pub fn search_highlight() -> Self {
        Self {
            fg: Some(TermColor::Rgb(36, 41, 47)),
            bg: Some(TermColor::Rgb(255, 235, 132)),
            bold: true,
            ..Self::default()
        }
    }

    pub fn status() -> Self {
        Self {
            fg: Some(TermColor::Rgb(36, 41, 47)),
            bg: Some(TermColor::Rgb(234, 238, 242)),
            ..Self::default()
        }
    }

    pub fn error() -> Self {
        Self {
            fg: Some(TermColor::Rgb(180, 35, 24)),
            bold: true,
            ..Self::default()
        }
    }

    pub fn sgr(&self) -> String {
        let mut params: Vec<String> = vec!["0".to_string()];
        if self.bold {
            params.push("1".to_string());
        }
        if self.dim {
            params.push("2".to_string());
        }
        if self.italic {
            params.push("3".to_string());
        }
        if self.underline {
            params.push("4".to_string());
        }
        if self.reverse {
            params.push("7".to_string());
        }
        if let Some(TermColor::Rgb(r, g, b)) = self.fg {
            params.push(format!("38;2;{r};{g};{b}"));
        }
        if let Some(TermColor::Rgb(r, g, b)) = self.bg {
            params.push(format!("48;2;{r};{g};{b}"));
        }
        format!("\x1b[{}m", params.join(";"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
}

impl Segment {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
            link: None,
        }
    }

    pub fn with_link(text: impl Into<String>, style: Style, link: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style,
            link: Some(link.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSlot {
    pub path: PathBuf,
    pub alt: String,
    pub width_cells: u16,
    pub height_cells: u16,
    pub original_width: Option<u32>,
    pub original_height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineContent {
    Text(Vec<Segment>),
    Image(ImageSlot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderLine {
    pub content: LineContent,
}

impl RenderLine {
    pub fn text(segments: Vec<Segment>) -> Self {
        Self {
            content: LineContent::Text(segments),
        }
    }

    pub fn blank() -> Self {
        Self::text(Vec::new())
    }

    pub fn image(slot: ImageSlot) -> Self {
        Self {
            content: LineContent::Image(slot),
        }
    }

    pub fn height(&self) -> usize {
        match &self.content {
            LineContent::Text(_) => 1,
            LineContent::Image(slot) => usize::from(slot.height_cells.max(1)),
        }
    }
}

pub fn plain_line(text: impl Into<String>) -> RenderLine {
    RenderLine::text(vec![Segment::new(text, Style::plain())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_sgr_uses_true_color_and_attributes() {
        let style = Style {
            fg: Some(TermColor::Rgb(1, 2, 3)),
            bg: Some(TermColor::Rgb(4, 5, 6)),
            bold: true,
            italic: true,
            underline: true,
            dim: false,
            reverse: false,
        };
        assert_eq!(style.sgr(), "\x1b[0;1;3;4;38;2;1;2;3;48;2;4;5;6m");
    }

    #[test]
    fn default_palette_is_light_theme_readable() {
        assert!(matches!(
            Style::heading(1).fg,
            Some(TermColor::Rgb(0, 90, 138))
        ));
        assert!(matches!(
            Style::link().fg,
            Some(TermColor::Rgb(9, 105, 218))
        ));
        assert!(matches!(
            Style::code().bg,
            Some(TermColor::Rgb(246, 248, 250))
        ));
        assert!(matches!(
            Style::code_string().fg,
            Some(TermColor::Rgb(17, 99, 41))
        ));
        assert!(matches!(
            Style::heading_banner().bg,
            Some(TermColor::Rgb(0, 90, 138))
        ));
        assert!(matches!(
            Style::table_header().bg,
            Some(TermColor::Rgb(232, 245, 237))
        ));
        assert!(matches!(
            Style::search_highlight().bg,
            Some(TermColor::Rgb(255, 235, 132))
        ));
        assert!(matches!(
            Style::status().bg,
            Some(TermColor::Rgb(234, 238, 242))
        ));
    }
}
