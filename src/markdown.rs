use crate::image::{build_image_slot, resolve_image_path};
use crate::rendered::{plain_line, ImageSlot, LineContent, RenderLine, Segment, Style};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};
use std::borrow::Cow;
use std::path::Path;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone)]
struct InlineState {
    style: Style,
    link: Option<String>,
}

#[derive(Debug, Clone)]
struct ListState {
    next_number: Option<u64>,
}

#[derive(Debug, Clone)]
struct PendingImage {
    dest: String,
    alt: String,
}

#[derive(Debug, Clone)]
struct HeadingState {
    level: u8,
    text: String,
}

struct Renderer<'a> {
    markdown_file: &'a Path,
    builder: LineBuilder,
    current: InlineState,
    stack: Vec<InlineState>,
    quote_depth: usize,
    lists: Vec<ListState>,
    code_block: bool,
    heading: Option<HeadingState>,
    pending_image: Option<PendingImage>,
}

pub fn render_markdown(markdown_file: &Path, source: &str, width: u16) -> Vec<RenderLine> {
    Renderer::new(markdown_file, width).render(source)
}

impl<'a> Renderer<'a> {
    fn new(markdown_file: &'a Path, width: u16) -> Self {
        Self {
            markdown_file,
            builder: LineBuilder::new(width as usize),
            current: InlineState {
                style: Style::plain(),
                link: None,
            },
            stack: Vec::new(),
            quote_depth: 0,
            lists: Vec::new(),
            code_block: false,
            heading: None,
            pending_image: None,
        }
    }

    fn render(mut self, source: &str) -> Vec<RenderLine> {
        let parser = Parser::new(source);
        self.apply_prefix();

        for event in parser {
            if self.handle_heading_event(&event) {
                continue;
            }

            if self.handle_pending_image_event(&event) {
                continue;
            }

            match event {
                Event::Start(tag) => self.start_tag(tag),
                Event::End(end) => self.end_tag(end),
                Event::Text(text) => {
                    if self.code_block {
                        self.builder
                            .append_preserved(&sanitize(&text), Style::code());
                    } else {
                        self.builder.append_wrapped(
                            &sanitize(&text),
                            &self.current.style,
                            &self.current.link,
                        );
                    }
                }
                Event::Code(code) => {
                    let mut style = self.current.style.clone();
                    let code_style = Style::code();
                    style.fg = code_style.fg;
                    style.bg = code_style.bg;
                    self.builder
                        .append_wrapped(&sanitize(&code), &style, &self.current.link);
                }
                Event::Html(html) | Event::InlineHtml(html) => {
                    let mut style = self.current.style.clone();
                    style.dim = true;
                    self.builder
                        .append_wrapped(&sanitize(&html), &style, &self.current.link);
                }
                Event::SoftBreak => {
                    self.builder
                        .append_wrapped(" ", &self.current.style, &self.current.link)
                }
                Event::HardBreak => self.builder.finish_line(),
                Event::Rule => {
                    self.builder.finish_line();
                    let mut style = Style::plain();
                    style.dim = true;
                    self.builder.push_line(RenderLine::text(vec![Segment::new(
                        "-".repeat(self.builder.width),
                        style,
                    )]));
                    self.builder.blank_line();
                }
                Event::FootnoteReference(name) => self.builder.append_wrapped(
                    &format!("[{}]", sanitize(&name)),
                    &self.current.style,
                    &self.current.link,
                ),
                Event::TaskListMarker(checked) => {
                    let marker = if checked { "[x] " } else { "[ ] " };
                    self.builder
                        .append_wrapped(marker, &self.current.style, &self.current.link);
                }
                Event::InlineMath(math) | Event::DisplayMath(math) => {
                    self.builder.append_wrapped(
                        &format!("${}$", sanitize(&math)),
                        &self.current.style,
                        &self.current.link,
                    );
                }
            }
        }

        self.builder.finish_line();
        self.builder.lines
    }

    fn handle_heading_event(&mut self, event: &Event<'_>) -> bool {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.heading = Some(HeadingState {
                    level: heading_level(*level),
                    text: String::new(),
                });
                true
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = self.heading.take() {
                    self.finish_heading(heading);
                    true
                } else {
                    false
                }
            }
            Event::Text(text)
            | Event::Code(text)
            | Event::Html(text)
            | Event::InlineHtml(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => {
                if let Some(heading) = &mut self.heading {
                    heading.text.push_str(&sanitize(text));
                    true
                } else {
                    false
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(heading) = &mut self.heading {
                    heading.text.push(' ');
                    true
                } else {
                    false
                }
            }
            _ => self.heading.is_some(),
        }
    }

    fn finish_heading(&mut self, heading: HeadingState) {
        self.builder.finish_line();
        let title = collapse_spaces(&heading.text);
        let title = if title.is_empty() {
            "Untitled section".to_string()
        } else {
            title
        };

        match heading.level {
            1 => {
                if !self.builder.lines.is_empty() {
                    self.builder.blank_line();
                }
                self.builder.push_line(RenderLine::text(vec![Segment::new(
                    centered_heading(&title, self.builder.width, 2),
                    Style::heading_banner(),
                )]));
                self.builder.blank_line();
            }
            2 => {
                if !self.builder.lines.is_empty() {
                    self.builder.blank_line();
                }
                self.builder.push_line(RenderLine::text(vec![Segment::new(
                    "-".repeat(self.builder.width),
                    Style::heading_rule(2),
                )]));
                self.builder.push_line(RenderLine::text(vec![Segment::new(
                    padded_heading(&title, self.builder.width, 2),
                    Style::heading_panel(2),
                )]));
                self.builder.blank_line();
            }
            3 => {
                self.builder.push_line(RenderLine::text(vec![Segment::new(
                    padded_heading(&title, self.builder.width, 1),
                    Style::heading_panel(3),
                )]));
                self.builder.blank_line();
            }
            level => {
                self.builder.push_line(RenderLine::text(vec![Segment::new(
                    format!("{} {}", "#".repeat(level as usize), title),
                    Style::heading(level),
                )]));
                self.builder.blank_line();
            }
        }
    }

    fn handle_pending_image_event(&mut self, event: &Event<'_>) -> bool {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.pending_image = Some(PendingImage {
                    dest: dest_url.to_string(),
                    alt: String::new(),
                });
                true
            }
            Event::End(TagEnd::Image) => {
                if let Some(image) = self.pending_image.take() {
                    self.finish_image(image);
                    true
                } else {
                    false
                }
            }
            Event::Text(text)
            | Event::Code(text)
            | Event::Html(text)
            | Event::InlineHtml(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => {
                if let Some(image) = &mut self.pending_image {
                    image.alt.push_str(&sanitize(text));
                    true
                } else {
                    false
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(image) = &mut self.pending_image {
                    image.alt.push(' ');
                    true
                } else {
                    false
                }
            }
            _ => self.pending_image.is_some(),
        }
    }

    fn finish_image(&mut self, image: PendingImage) {
        self.builder.finish_line();
        let alt = if image.alt.trim().is_empty() {
            image.dest.clone()
        } else {
            image.alt.trim().to_string()
        };

        match resolve_image_path(self.markdown_file, &image.dest)
            .map(|path| build_image_slot(path, alt.clone(), self.builder.width as u16))
        {
            Some(Ok(slot)) => self.builder.push_line(RenderLine::image(slot)),
            Some(Err(err)) => {
                let mut style = Style::error();
                style.bold = false;
                self.builder.push_line(RenderLine::text(vec![Segment::new(
                    format!("[image: {alt}] {} ({err})", image.dest),
                    style,
                )]));
            }
            None => self
                .builder
                .push_line(plain_line(format!("[image: {alt}] {}", image.dest))),
        }
        self.builder.blank_line();
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.builder.finish_line();
                self.push_style(|style| *style = Style::heading(heading_level(level)));
                self.builder.append_wrapped(
                    &"#".repeat(heading_level(level) as usize),
                    &self.current.style,
                    &self.current.link,
                );
                self.builder
                    .append_wrapped(" ", &self.current.style, &self.current.link);
            }
            Tag::BlockQuote(_) => {
                self.builder.finish_line();
                self.quote_depth += 1;
                self.push_style(|style| {
                    let quote = Style::quote();
                    style.fg = quote.fg;
                    style.italic = true;
                });
                self.apply_prefix();
            }
            Tag::CodeBlock(kind) => {
                self.builder.finish_line();
                self.code_block = true;
                self.apply_code_prefix();
                if let CodeBlockKind::Fenced(lang) = kind {
                    if !lang.trim().is_empty() {
                        let mut style = Style::code();
                        style.dim = true;
                        self.builder
                            .append_preserved(&format!("// {}", sanitize(&lang)), style);
                    }
                }
            }
            Tag::List(start) => {
                self.builder.finish_line();
                self.lists.push(ListState { next_number: start });
                self.apply_prefix();
            }
            Tag::Item => self.start_item(),
            Tag::Emphasis => self.push_style(|style| style.italic = true),
            Tag::Strong => self.push_style(|style| style.bold = true),
            Tag::Strikethrough => self.push_style(|style| style.dim = true),
            Tag::Superscript | Tag::Subscript => self.push_style(|style| style.italic = true),
            Tag::Link { dest_url, .. } => self.push_link(dest_url.to_string()),
            Tag::Image { .. } => {}
            Tag::HtmlBlock => self.push_style(|style| style.dim = true),
            Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::FootnoteDefinition(_)
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end_tag(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph => {
                self.builder.finish_line();
                self.builder.blank_line();
            }
            TagEnd::Heading(_) => {
                self.builder.finish_line();
                self.builder.blank_line();
                self.pop_style();
            }
            TagEnd::BlockQuote(_) => {
                self.builder.finish_line();
                self.pop_style();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.apply_prefix();
                self.builder.blank_line();
            }
            TagEnd::CodeBlock => {
                self.builder.finish_line();
                self.code_block = false;
                self.apply_prefix();
                self.builder.blank_line();
            }
            TagEnd::List(_) => {
                self.builder.finish_line();
                self.lists.pop();
                self.apply_prefix();
                self.builder.blank_line();
            }
            TagEnd::Item => {
                self.builder.finish_line();
                self.apply_prefix();
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::Link => self.pop_style(),
            TagEnd::HtmlBlock => self.pop_style(),
            TagEnd::Image
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    fn start_item(&mut self) {
        self.builder.finish_line();
        let quote = quote_prefix(self.quote_depth);
        let indent = "  ".repeat(self.lists.len().saturating_sub(1));
        let marker = if let Some(list) = self.lists.last_mut() {
            if let Some(number) = &mut list.next_number {
                let marker = format!("{number}. ");
                *number = number.saturating_add(1);
                marker
            } else {
                "- ".to_string()
            }
        } else {
            "- ".to_string()
        };
        let marker_width = UnicodeWidthStr::width(marker.as_str());
        let mut prefix_style = Style::plain();
        prefix_style.dim = true;
        let first = vec![Segment::new(
            format!("{quote}{indent}{marker}"),
            prefix_style.clone(),
        )];
        let rest = vec![Segment::new(
            format!("{quote}{indent}{}", " ".repeat(marker_width)),
            prefix_style,
        )];
        self.builder.set_prefix(Some(first), rest);
    }

    fn apply_prefix(&mut self) {
        let mut style = if self.quote_depth > 0 {
            Style::quote()
        } else {
            Style::plain()
        };
        style.dim = self.quote_depth > 0;
        let prefix = format!(
            "{}{}",
            quote_prefix(self.quote_depth),
            "  ".repeat(self.lists.len())
        );
        if prefix.is_empty() {
            self.builder.set_prefix(None, Vec::new());
        } else {
            self.builder
                .set_prefix(None, vec![Segment::new(prefix, style)]);
        }
    }

    fn apply_code_prefix(&mut self) {
        let prefix = format!("{}  ", quote_prefix(self.quote_depth));
        let mut style = Style::code();
        style.dim = true;
        self.builder
            .set_prefix(None, vec![Segment::new(prefix, style)]);
    }

    fn push_style<F>(&mut self, change: F)
    where
        F: FnOnce(&mut Style),
    {
        self.stack.push(self.current.clone());
        change(&mut self.current.style);
    }

    fn push_link(&mut self, link: String) {
        self.stack.push(self.current.clone());
        let link_style = Style::link();
        self.current.style.fg = link_style.fg;
        self.current.style.underline = true;
        self.current.link = Some(link);
    }

    fn pop_style(&mut self) {
        if let Some(previous) = self.stack.pop() {
            self.current = previous;
        }
    }
}

struct LineBuilder {
    width: usize,
    lines: Vec<RenderLine>,
    current: Vec<Segment>,
    col: usize,
    current_prefix_width: usize,
    pending_prefix: Option<Vec<Segment>>,
    normal_prefix: Vec<Segment>,
}

impl LineBuilder {
    fn new(width: usize) -> Self {
        Self {
            width: width.max(1),
            lines: Vec::new(),
            current: Vec::new(),
            col: 0,
            current_prefix_width: 0,
            pending_prefix: None,
            normal_prefix: Vec::new(),
        }
    }

    fn set_prefix(&mut self, pending: Option<Vec<Segment>>, normal: Vec<Segment>) {
        self.pending_prefix = pending;
        self.normal_prefix = normal;
    }

    fn append_wrapped(&mut self, text: &str, style: &Style, link: &Option<String>) {
        for token in tokenize_wrapped(text) {
            match token {
                WrapToken::Newline => self.finish_line(),
                WrapToken::Space => self.append_space(style, link),
                WrapToken::Word(word) => self.append_word(&word, style, link),
            }
        }
    }

    fn append_preserved(&mut self, text: &str, style: Style) {
        for ch in text.chars() {
            if ch == '\n' {
                self.finish_line();
                continue;
            }
            self.ensure_line();
            let width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width > 0 && self.col + width > self.width && self.col > self.current_prefix_width {
                self.finish_line();
                self.ensure_line();
            }
            self.push_text(&ch.to_string(), style.clone(), None);
            self.col += width;
        }
    }

    fn append_space(&mut self, style: &Style, link: &Option<String>) {
        self.ensure_line();
        if self.col == self.current_prefix_width {
            return;
        }
        if self.col + 1 > self.width {
            self.finish_line();
            return;
        }
        self.push_text(" ", style.clone(), link.clone());
        self.col += 1;
    }

    fn append_word(&mut self, word: &str, style: &Style, link: &Option<String>) {
        let word_width = UnicodeWidthStr::width(word);
        self.ensure_line();

        if word_width > 0
            && self.col + word_width > self.width
            && self.col > self.current_prefix_width
        {
            self.finish_line();
            self.ensure_line();
        }

        if word_width <= self.width.saturating_sub(self.col) {
            self.push_text(word, style.clone(), link.clone());
            self.col += word_width;
            return;
        }

        for ch in word.chars() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if char_width > 0
                && self.col + char_width > self.width
                && self.col > self.current_prefix_width
            {
                self.finish_line();
                self.ensure_line();
            }
            self.push_text(&ch.to_string(), style.clone(), link.clone());
            self.col += char_width;
        }
    }

    fn ensure_line(&mut self) {
        if !self.current.is_empty() || self.col != 0 {
            return;
        }
        let prefix = self
            .pending_prefix
            .take()
            .unwrap_or_else(|| self.normal_prefix.clone());
        self.current_prefix_width = line_width(&prefix);
        self.col = self.current_prefix_width;
        self.current = prefix;
    }

    fn finish_line(&mut self) {
        if self.current.is_empty() && self.col == 0 {
            return;
        }
        let current = std::mem::take(&mut self.current);
        self.lines.push(RenderLine::text(current));
        self.col = 0;
        self.current_prefix_width = 0;
    }

    fn blank_line(&mut self) {
        self.finish_line();
        if self.lines.last().is_some_and(|line| !is_blank(line)) {
            self.lines.push(RenderLine::blank());
        }
    }

    fn push_line(&mut self, line: RenderLine) {
        self.finish_line();
        self.lines.push(line);
    }

    fn push_text(&mut self, text: &str, style: Style, link: Option<String>) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.current.last_mut() {
            if last.style == style && last.link == link {
                last.text.push_str(text);
                return;
            }
        }
        self.current.push(Segment {
            text: text.to_string(),
            style,
            link,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WrapToken<'a> {
    Word(Cow<'a, str>),
    Space,
    Newline,
}

fn tokenize_wrapped(text: &str) -> Vec<WrapToken<'_>> {
    let mut tokens = Vec::new();
    let mut start = None;

    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            if let Some(word_start) = start.take() {
                tokens.push(WrapToken::Word(Cow::Borrowed(&text[word_start..idx])));
            }
            tokens.push(WrapToken::Newline);
        } else if ch.is_whitespace() {
            if let Some(word_start) = start.take() {
                tokens.push(WrapToken::Word(Cow::Borrowed(&text[word_start..idx])));
            }
            if !matches!(tokens.last(), Some(WrapToken::Space | WrapToken::Newline)) {
                tokens.push(WrapToken::Space);
            }
        } else if start.is_none() {
            start = Some(idx);
        }
    }

    if let Some(word_start) = start {
        tokens.push(WrapToken::Word(Cow::Borrowed(&text[word_start..])));
    }

    tokens
}

fn line_width(segments: &[Segment]) -> usize {
    segments
        .iter()
        .map(|segment| UnicodeWidthStr::width(segment.text.as_str()))
        .sum()
}

fn is_blank(line: &RenderLine) -> bool {
    matches!(&line.content, LineContent::Text(segments) if segments.iter().all(|segment| segment.text.trim().is_empty()))
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn quote_prefix(depth: usize) -> String {
    "> ".repeat(depth)
}

fn collapse_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn centered_heading(title: &str, width: usize, min_side_padding: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let inner_width = width.saturating_sub(min_side_padding.saturating_mul(2));
    let fitted = fit_to_width(title, inner_width.max(1));
    let fitted_width = UnicodeWidthStr::width(fitted.as_str());
    let remaining = width.saturating_sub(fitted_width);
    let left = (remaining / 2).max(min_side_padding.min(width));
    let left = left.min(width.saturating_sub(fitted_width));
    let right = width.saturating_sub(left + fitted_width);
    format!("{}{}{}", " ".repeat(left), fitted, " ".repeat(right))
}

fn padded_heading(title: &str, width: usize, left_padding: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let left = left_padding.min(width);
    let fitted = fit_to_width(title, width.saturating_sub(left).max(1));
    let fitted_width = UnicodeWidthStr::width(fitted.as_str());
    let right = width.saturating_sub(left + fitted_width);
    format!("{}{}{}", " ".repeat(left), fitted, " ".repeat(right))
}

fn fit_to_width(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + char_width > width {
            break;
        }
        out.push(ch);
        used += char_width;
    }
    out
}

fn sanitize(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch == '\n' || ch == '\t' || !ch.is_control() {
                ch
            } else {
                ' '
            }
        })
        .collect()
}

pub fn total_rows(lines: &[RenderLine]) -> usize {
    lines.iter().map(RenderLine::height).sum()
}

pub fn flatten_text(lines: &[RenderLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match &line.content {
            LineContent::Text(segments) => segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect(),
            LineContent::Image(ImageSlot { alt, .. }) => format!("[image: {alt}]"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn render(source: &str, width: u16) -> Vec<String> {
        flatten_text(&render_markdown(Path::new("/tmp/doc.md"), source, width))
    }

    #[test]
    fn renders_heading_and_paragraph() {
        let lines = render("# Title\n\nHello **world**.", 80);
        assert_eq!(lines[0].trim(), "Title");
        assert_eq!(UnicodeWidthStr::width(lines[0].as_str()), 80);
        assert_eq!(lines[2], "Hello world.");
    }

    #[test]
    fn renders_section_titles_as_visual_blocks() {
        let lines = render_markdown(Path::new("/tmp/doc.md"), "# Main\n\n## Details", 32);
        let LineContent::Text(h1_segments) = &lines[0].content else {
            panic!("expected h1 text line");
        };
        assert_eq!(h1_segments[0].text.trim(), "Main");
        assert_eq!(h1_segments[0].style, Style::heading_banner());

        let LineContent::Text(rule_segments) = &lines[2].content else {
            panic!("expected h2 rule line");
        };
        assert_eq!(rule_segments[0].text, "-".repeat(32));
        assert_eq!(rule_segments[0].style, Style::heading_rule(2));

        let LineContent::Text(h2_segments) = &lines[3].content else {
            panic!("expected h2 title line");
        };
        assert_eq!(h2_segments[0].text.trim(), "Details");
        assert_eq!(h2_segments[0].style, Style::heading_panel(2));
    }

    #[test]
    fn wraps_text_on_words() {
        let lines = render("alpha beta gamma", 10);
        assert_eq!(lines, vec!["alpha beta", "gamma", ""]);
    }

    #[test]
    fn wraps_long_words_without_losing_text() {
        let lines = render("abcdefghijkl", 5);
        assert_eq!(lines, vec!["abcde", "fghij", "kl", ""]);
    }

    #[test]
    fn renders_lists_and_quotes() {
        let lines = render("> quote\n\n1. one\n2. two", 80);
        assert!(lines.contains(&"> quote".to_string()));
        assert!(lines.contains(&"1. one".to_string()));
        assert!(lines.contains(&"2. two".to_string()));
    }

    #[test]
    fn renders_quote_body_in_italic() {
        let lines = render_markdown(
            Path::new("/tmp/doc.md"),
            "> quote [link](https://example.test)",
            80,
        );
        let LineContent::Text(segments) = &lines[0].content else {
            panic!("expected quote text line");
        };

        let quote = segments
            .iter()
            .find(|segment| segment.text.contains("quote"))
            .expect("quote body segment");
        assert!(quote.style.italic);
        assert_eq!(quote.style.fg, Style::quote().fg);

        let link = segments
            .iter()
            .find(|segment| segment.text.contains("link"))
            .expect("quote link segment");
        assert!(link.style.italic);
        assert!(link.style.underline);
        assert_eq!(link.link.as_deref(), Some("https://example.test"));
    }

    #[test]
    fn renders_code_blocks_with_language_header() {
        let lines = render("```rust\nfn main() {}\n```", 80);
        assert!(lines.iter().any(|line| line.contains("// rust")));
        assert!(lines.iter().any(|line| line.contains("fn main() {}")));
    }

    #[test]
    fn keeps_link_metadata_on_link_segments() {
        let lines = render_markdown(
            Path::new("/tmp/doc.md"),
            "[Rust](https://rust-lang.org)",
            80,
        );
        let LineContent::Text(segments) = &lines[0].content else {
            panic!("expected text line");
        };
        assert_eq!(segments[0].text, "Rust");
        assert_eq!(segments[0].link.as_deref(), Some("https://rust-lang.org"));
        assert!(segments[0].style.underline);
    }

    #[test]
    fn turns_local_images_into_image_slots() {
        let dir = std::env::temp_dir().join(format!(
            "mdview-image-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("sample.ppm");
        fs::write(&image_path, b"P3\n4 2\n255\n0 0 0\n").unwrap();
        let md_path = dir.join("doc.md");
        let lines = render_markdown(&md_path, "![Alt](sample.ppm)", 40);
        assert!(matches!(lines[0].content, LineContent::Image(_)));
        let _ = fs::remove_file(image_path);
        let _ = fs::remove_dir(dir);
    }

    #[test]
    fn sanitizes_escape_controls() {
        let lines = render("hello \x1b[31mred", 80);
        assert_eq!(lines[0], "hello [31mred");
    }
}
