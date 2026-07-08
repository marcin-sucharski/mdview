use crate::rendered::{Segment, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyKind {
    Json,
    Text,
    Xml,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpState {
    Start,
    Headers,
    Body(BodyKind),
}

pub fn highlight_code(language: &str, source: &str) -> Vec<Vec<Segment>> {
    let language = normalize_language(language);
    match language.as_str() {
        "json" => highlight_json_lines(source),
        "http" | "httpspec" => highlight_http_lines(source),
        "xml" | "html" => highlight_xml_lines(source),
        "text" | "txt" | "plain" | "plaintext" | "text/plain" => plain_lines(source),
        _ => plain_lines(source),
    }
}

fn normalize_language(language: &str) -> String {
    language
        .split(|ch: char| ch.is_ascii_whitespace() || ch == '{' || ch == ',')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

fn source_lines(source: &str) -> Vec<&str> {
    if source.is_empty() {
        return vec![""];
    }

    let mut lines = source.split('\n').collect::<Vec<_>>();
    if source.ends_with('\n') {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push("");
    }
    lines
}

fn plain_lines(source: &str) -> Vec<Vec<Segment>> {
    source_lines(source)
        .into_iter()
        .map(|line| vec![Segment::new(line, Style::code())])
        .collect()
}

fn highlight_json_lines(source: &str) -> Vec<Vec<Segment>> {
    source_lines(source)
        .into_iter()
        .map(highlight_json_line)
        .collect()
}

fn highlight_json_line(line: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut chars = line.char_indices().peekable();

    while let Some((start, ch)) = chars.next() {
        if ch == '"' {
            let end = json_string_end(line, &mut chars);
            let token = &line[start..end];
            let style = if json_string_is_key(line, end) {
                Style::code_key()
            } else {
                Style::code_string()
            };
            push_segment(&mut segments, token, style);
        } else if ch == '-' || ch.is_ascii_digit() {
            let end = json_number_end(start, ch, &mut chars);
            push_segment(&mut segments, &line[start..end], Style::code_number());
        } else if is_json_word_start(ch) {
            let end = word_end(line, start);
            let token = &line[start..end];
            let style = if matches!(token, "true" | "false" | "null") {
                Style::code_literal()
            } else {
                Style::code()
            };
            push_segment(&mut segments, token, style);
            while let Some((idx, next)) = chars.peek() {
                if *idx < end && *next != '\0' {
                    let _ = chars.next();
                } else {
                    break;
                }
            }
        } else if matches!(ch, '{' | '}' | '[' | ']' | ':' | ',') {
            push_segment(
                &mut segments,
                &line[start..start + ch.len_utf8()],
                Style::code_punctuation(),
            );
        } else {
            push_segment(
                &mut segments,
                &line[start..start + ch.len_utf8()],
                Style::code(),
            );
        }
    }

    if segments.is_empty() {
        segments.push(Segment::new("", Style::code()));
    }
    segments
}

fn json_string_end(
    line: &str,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    let mut escaped = false;
    let mut end = line.len();
    for (idx, ch) in chars.by_ref() {
        end = idx + ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            break;
        }
    }
    end
}

fn json_string_is_key(line: &str, end: usize) -> bool {
    line[end..]
        .chars()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| ch == ':')
}

fn json_number_end(
    start: usize,
    first: char,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> usize {
    let mut end = start + first.len_utf8();
    while let Some((idx, ch)) = chars.peek().copied() {
        if ch.is_ascii_digit() || matches!(ch, '.' | 'e' | 'E' | '+' | '-') {
            let _ = chars.next();
            end = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn is_json_word_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn word_end(line: &str, start: usize) -> usize {
    line[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (!ch.is_ascii_alphanumeric() && ch != '_' && offset > 0).then_some(start + offset)
        })
        .unwrap_or(line.len())
}

fn highlight_http_lines(source: &str) -> Vec<Vec<Segment>> {
    let mut state = HttpState::Start;
    let mut content_type = BodyKind::Unknown;
    let mut out = Vec::new();

    for line in source_lines(source) {
        if line.trim() == ">>>" {
            out.push(vec![Segment::new(line, Style::code_comment())]);
            state = HttpState::Start;
            content_type = BodyKind::Unknown;
            continue;
        }

        match state {
            HttpState::Start => {
                if line.trim().is_empty() {
                    out.push(vec![Segment::new(line, Style::code())]);
                } else {
                    out.push(highlight_http_start_line(line));
                    state = HttpState::Headers;
                }
            }
            HttpState::Headers => {
                if line.trim().is_empty() {
                    out.push(vec![Segment::new(line, Style::code())]);
                    state = HttpState::Body(content_type);
                } else if let Some((name, value)) = split_header(line) {
                    if name.eq_ignore_ascii_case("content-type") {
                        content_type = body_kind_from_content_type(value);
                    }
                    out.push(highlight_http_header(line, name, value));
                } else {
                    out.push(vec![Segment::new(line, Style::code())]);
                }
            }
            HttpState::Body(kind) => {
                out.push(highlight_body_line(line, kind));
            }
        }
    }

    if out.is_empty() {
        out.push(vec![Segment::new("", Style::code())]);
    }
    out
}

fn highlight_http_start_line(line: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let Some((first, rest)) = line.split_once(' ') else {
        push_segment(&mut segments, line, Style::code_keyword());
        return segments;
    };

    push_segment(&mut segments, first, Style::code_keyword());
    push_segment(&mut segments, " ", Style::code());

    if first.starts_with("HTTP") {
        let Some((status, reason)) = rest.split_once(' ') else {
            push_segment(&mut segments, rest, Style::code_number());
            return segments;
        };
        push_segment(&mut segments, status, Style::code_number());
        push_segment(&mut segments, " ", Style::code());
        push_segment(&mut segments, reason, Style::code());
    } else {
        push_segment(&mut segments, rest, Style::code());
    }

    segments
}

fn split_header(line: &str) -> Option<(&str, &str)> {
    let (name, value) = line.split_once(':')?;
    (!name.trim().is_empty()).then_some((name, value))
}

fn highlight_http_header(line: &str, name: &str, value: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    push_segment(&mut segments, name, Style::code_key());
    push_segment(&mut segments, ":", Style::code_punctuation());
    let value_start = name.len() + 1;
    push_segment(
        &mut segments,
        &line[value_start..value_start + leading_ws(value).len()],
        Style::code(),
    );
    push_segment(&mut segments, value.trim_start(), Style::code_string());
    segments
}

fn leading_ws(text: &str) -> &str {
    let end = text
        .char_indices()
        .find_map(|(idx, ch)| (!ch.is_whitespace()).then_some(idx))
        .unwrap_or(text.len());
    &text[..end]
}

fn body_kind_from_content_type(content_type: &str) -> BodyKind {
    let content_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if content_type == "application/json" || content_type.ends_with("+json") {
        BodyKind::Json
    } else if matches!(content_type.as_str(), "application/xml" | "text/xml")
        || content_type.ends_with("+xml")
    {
        BodyKind::Xml
    } else if content_type.starts_with("text/") {
        BodyKind::Text
    } else {
        BodyKind::Unknown
    }
}

fn highlight_body_line(line: &str, kind: BodyKind) -> Vec<Segment> {
    match kind {
        BodyKind::Json => highlight_json_line(line),
        BodyKind::Xml => highlight_xml_line(line),
        BodyKind::Text | BodyKind::Unknown => vec![Segment::new(line, Style::code())],
    }
}

fn highlight_xml_lines(source: &str) -> Vec<Vec<Segment>> {
    source_lines(source)
        .into_iter()
        .map(highlight_xml_line)
        .collect()
}

fn highlight_xml_line(line: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut i = 0usize;

    while i < line.len() {
        let rest = &line[i..];
        if rest.starts_with("<!--") {
            let end = rest
                .find("-->")
                .map(|idx| i + idx + 3)
                .unwrap_or(line.len());
            push_segment(&mut segments, &line[i..end], Style::code_comment());
            i = end;
        } else if rest.starts_with("<![CDATA[") {
            let end = rest
                .find("]]>")
                .map(|idx| i + idx + 3)
                .unwrap_or(line.len());
            push_segment(&mut segments, &line[i..end], Style::code_string());
            i = end;
        } else if rest.starts_with('<') {
            let end = rest.find('>').map(|idx| i + idx + 1).unwrap_or(line.len());
            highlight_xml_tag(&mut segments, &line[i..end]);
            i = end;
        } else {
            let end = rest.find('<').map(|idx| i + idx).unwrap_or(line.len());
            push_segment(&mut segments, &line[i..end], Style::code());
            i = end;
        }
    }

    if segments.is_empty() {
        segments.push(Segment::new("", Style::code()));
    }
    segments
}

fn highlight_xml_tag(segments: &mut Vec<Segment>, tag: &str) {
    let mut i = 0usize;
    let bytes = tag.as_bytes();
    while i < tag.len() {
        let ch = tag[i..].chars().next().unwrap_or_default();
        if matches!(ch, '<' | '>' | '/' | '=') {
            push_segment(
                segments,
                &tag[i..i + ch.len_utf8()],
                Style::code_punctuation(),
            );
            i += ch.len_utf8();
        } else if ch.is_whitespace() {
            let end = scan_while(tag, i, char::is_whitespace);
            push_segment(segments, &tag[i..end], Style::code());
            i = end;
        } else if ch == '"' || ch == '\'' {
            let end = quoted_value_end(tag, i, ch);
            push_segment(segments, &tag[i..end], Style::code_string());
            i = end;
        } else {
            let end = scan_xml_name(bytes, i);
            let style = if previous_non_ws(tag, i).is_some_and(|prev| prev == '<' || prev == '/') {
                Style::code_keyword()
            } else {
                Style::code_key()
            };
            push_segment(segments, &tag[i..end], style);
            i = end;
        }
    }
}

fn scan_while<F>(text: &str, start: usize, mut predicate: F) -> usize
where
    F: FnMut(char) -> bool,
{
    text[start..]
        .char_indices()
        .find_map(|(offset, ch)| (!predicate(ch)).then_some(start + offset))
        .unwrap_or(text.len())
}

fn scan_xml_name(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len()
        && !matches!(
            bytes[end],
            b'<' | b'>' | b'/' | b'=' | b' ' | b'\t' | b'\r' | b'\n'
        )
    {
        end += 1;
    }
    end.max(start + 1).min(bytes.len())
}

fn quoted_value_end(text: &str, start: usize, quote: char) -> usize {
    let mut escaped = false;
    for (offset, ch) in text[start + quote.len_utf8()..].char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return start + quote.len_utf8() + offset + quote.len_utf8();
        }
    }
    text.len()
}

fn previous_non_ws(text: &str, idx: usize) -> Option<char> {
    text[..idx].chars().rev().find(|ch| !ch.is_whitespace())
}

fn push_segment(segments: &mut Vec<Segment>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = segments.last_mut() {
        if last.style == style && last.link.is_none() {
            last.text.push_str(text);
            return;
        }
    }
    segments.push(Segment::new(text, style));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_style<'a>(lines: &'a [Vec<Segment>], text: &str) -> &'a Style {
        &lines
            .iter()
            .flat_map(|line| line.iter())
            .find(|segment| segment.text == text)
            .unwrap_or_else(|| panic!("missing segment {text:?}"))
            .style
    }

    #[test]
    fn highlights_json_tokens() {
        let lines = highlight_code("json", r#"{"ok": true, "count": 2}"#);
        assert_eq!(*find_style(&lines, r#""ok""#), Style::code_key());
        assert_eq!(*find_style(&lines, "true"), Style::code_literal());
        assert_eq!(*find_style(&lines, "2"), Style::code_number());
    }

    #[test]
    fn highlights_http_json_bodies_by_content_type() {
        let source = "POST /items HTTP/1.1\nContent-Type: application/json\n\n{\"ok\": true}\n>>>\nHTTP 200 OK\nContent-Type: application/json\n\n{\"id\": 2}";
        let lines = highlight_code("http", source);
        assert_eq!(*find_style(&lines, "POST"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "Content-Type"), Style::code_key());
        assert_eq!(*find_style(&lines, r#""ok""#), Style::code_key());
        assert_eq!(*find_style(&lines, "HTTP"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "200"), Style::code_number());
        assert_eq!(*find_style(&lines, r#""id""#), Style::code_key());
    }

    #[test]
    fn highlights_xml_tokens() {
        let lines = highlight_code("xml", r#"<item id="1">text</item>"#);
        assert_eq!(*find_style(&lines, "item"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "id"), Style::code_key());
        assert_eq!(*find_style(&lines, r#""1""#), Style::code_string());
    }

    #[test]
    fn leaves_plain_text_as_code() {
        let lines = highlight_code("text", "plain text");
        assert_eq!(lines[0][0], Segment::new("plain text", Style::code()));
    }
}
