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

#[derive(Debug, Clone, PartialEq, Eq)]
enum SqlState {
    Normal,
    BlockComment(usize),
    DollarString(String),
}

pub fn highlight_code(language: &str, source: &str) -> Vec<Vec<Segment>> {
    let language = normalize_language(language);
    match language.as_str() {
        "json" => highlight_json_lines(source),
        "http" | "httpspec" => highlight_http_lines(source),
        "sql" | "postgres" | "postgresql" | "pgsql" | "psql" | "plpgsql" => {
            highlight_sql_lines(source)
        }
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

fn highlight_sql_lines(source: &str) -> Vec<Vec<Segment>> {
    let mut state = SqlState::Normal;
    source_lines(source)
        .into_iter()
        .map(|line| highlight_sql_line(line, &mut state))
        .collect()
}

fn highlight_sql_line(line: &str, state: &mut SqlState) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut i = 0usize;

    while i < line.len() {
        match state {
            SqlState::BlockComment(depth) => {
                let (end, next_depth) = sql_block_comment_end(line, i, *depth);
                push_segment(&mut segments, &line[i..end], Style::code_comment());
                if next_depth == 0 {
                    *state = SqlState::Normal;
                } else {
                    *depth = next_depth;
                }
                i = end;
            }
            SqlState::DollarString(delimiter) => {
                if let Some(end) = line[i..].find(delimiter.as_str()) {
                    let end = i + end + delimiter.len();
                    push_segment(&mut segments, &line[i..end], Style::code_string());
                    *state = SqlState::Normal;
                    i = end;
                } else {
                    push_segment(&mut segments, &line[i..], Style::code_string());
                    i = line.len();
                }
            }
            SqlState::Normal => {
                let rest = &line[i..];
                let ch = rest.chars().next().unwrap_or_default();

                if ch.is_whitespace() {
                    let end = scan_while(line, i, char::is_whitespace);
                    push_segment(&mut segments, &line[i..end], Style::code());
                    i = end;
                } else if rest.starts_with("--") {
                    push_segment(&mut segments, rest, Style::code_comment());
                    i = line.len();
                } else if rest.starts_with("/*") {
                    let (end, depth) = sql_block_comment_end(line, i, 0);
                    push_segment(&mut segments, &line[i..end], Style::code_comment());
                    if depth > 0 {
                        *state = SqlState::BlockComment(depth);
                    }
                    i = end;
                } else if let Some(delimiter) = sql_dollar_quote_delimiter(rest) {
                    let search_start = i + delimiter.len();
                    if let Some(close) = line[search_start..].find(delimiter.as_str()) {
                        let end = search_start + close + delimiter.len();
                        push_segment(&mut segments, &line[i..end], Style::code_string());
                        i = end;
                    } else {
                        push_segment(&mut segments, &line[i..], Style::code_string());
                        *state = SqlState::DollarString(delimiter);
                        i = line.len();
                    }
                } else if let Some(end) = sql_prefixed_string_end(line, i) {
                    push_segment(&mut segments, &line[i..end], Style::code_string());
                    i = end;
                } else if ch == '\'' {
                    let end = sql_quoted_end(line, i, '\'', false);
                    push_segment(&mut segments, &line[i..end], Style::code_string());
                    i = end;
                } else if ch == '"' {
                    let end = sql_quoted_end(line, i, '"', false);
                    push_segment(&mut segments, &line[i..end], Style::code_key());
                    i = end;
                } else if ch == '$' && rest[1..].starts_with(|next: char| next.is_ascii_digit()) {
                    let end = i + 1 + scan_ascii_digits(&rest[1..]);
                    push_segment(&mut segments, &line[i..end], Style::code_number());
                    i = end;
                } else if ch.is_ascii_digit() {
                    let end = sql_number_end(line, i);
                    push_segment(&mut segments, &line[i..end], Style::code_number());
                    i = end;
                } else if is_sql_identifier_start(ch) {
                    let end = sql_identifier_end(line, i);
                    let token = &line[i..end];
                    let style = sql_identifier_style(line, end, token);
                    push_segment(&mut segments, token, style);
                    i = end;
                } else if let Some(len) = sql_operator_len(rest) {
                    push_segment(&mut segments, &line[i..i + len], Style::code_punctuation());
                    i += len;
                } else {
                    push_segment(&mut segments, &line[i..i + ch.len_utf8()], Style::code());
                    i += ch.len_utf8();
                }
            }
        }
    }

    if segments.is_empty() {
        segments.push(Segment::new("", Style::code()));
    }
    segments
}

fn sql_block_comment_end(line: &str, start: usize, mut depth: usize) -> (usize, usize) {
    let mut i = start;
    while i < line.len() {
        let rest = &line[i..];
        if rest.starts_with("/*") {
            depth += 1;
            i += 2;
        } else if rest.starts_with("*/") {
            depth = depth.saturating_sub(1);
            i += 2;
            if depth == 0 {
                return (i, 0);
            }
        } else {
            i += rest.chars().next().unwrap_or_default().len_utf8();
        }
    }
    (line.len(), depth)
}

fn sql_dollar_quote_delimiter(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.first().copied() != Some(b'$') {
        return None;
    }

    let mut end = 1usize;
    while end < bytes.len() && is_sql_dollar_tag_byte(bytes[end]) {
        end += 1;
    }
    (end < bytes.len() && bytes[end] == b'$').then(|| text[..=end].to_string())
}

fn is_sql_dollar_tag_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn sql_prefixed_string_end(line: &str, start: usize) -> Option<usize> {
    let rest = &line[start..];
    let lower = rest.to_ascii_lowercase();
    let (quote_offset, backslash_escape) = if lower.starts_with("e'") || lower.starts_with("b'") {
        (1, lower.starts_with("e'"))
    } else if lower.starts_with("u&'") {
        (2, true)
    } else {
        return None;
    };
    Some(sql_quoted_end(
        line,
        start + quote_offset,
        '\'',
        backslash_escape,
    ))
}

fn sql_quoted_end(text: &str, start: usize, quote: char, backslash_escape: bool) -> usize {
    let mut escaped = false;
    let mut i = start + quote.len_utf8();

    while i < text.len() {
        let ch = text[i..].chars().next().unwrap_or_default();
        if backslash_escape && escaped {
            escaped = false;
            i += ch.len_utf8();
        } else if backslash_escape && ch == '\\' {
            escaped = true;
            i += ch.len_utf8();
        } else if ch == quote {
            let next = i + quote.len_utf8();
            if text[next..].starts_with(quote) {
                i = next + quote.len_utf8();
            } else {
                return next;
            }
        } else {
            i += ch.len_utf8();
        }
    }

    text.len()
}

fn scan_ascii_digits(text: &str) -> usize {
    text.as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count()
}

fn sql_number_end(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut end = start + scan_ascii_digits(&line[start..]);

    if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
        end += 1 + scan_ascii_digits(&line[end + 1..]);
    }

    if bytes
        .get(end)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        let exponent = end + 1;
        let digits = if bytes
            .get(exponent)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            exponent + 1
        } else {
            exponent
        };
        let digit_count = scan_ascii_digits(&line[digits..]);
        if digit_count > 0 {
            end = digits + digit_count;
        }
    }

    end
}

fn is_sql_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_sql_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$')
}

fn sql_identifier_end(line: &str, start: usize) -> usize {
    line[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (!is_sql_identifier_char(ch) && offset > 0).then_some(start + offset)
        })
        .unwrap_or(line.len())
}

fn sql_identifier_style(line: &str, end: usize, token: &str) -> Style {
    let lower = token.to_ascii_lowercase();
    if is_sql_literal(&lower) {
        Style::code_literal()
    } else if is_sql_keyword(&lower) {
        Style::code_keyword()
    } else if is_postgres_type(&lower) || next_non_ws(line, end).is_some_and(|ch| ch == '(') {
        Style::code_key()
    } else {
        Style::code()
    }
}

fn next_non_ws(text: &str, idx: usize) -> Option<char> {
    text[idx..].chars().find(|ch| !ch.is_whitespace())
}

fn is_sql_literal(token: &str) -> bool {
    matches!(token, "false" | "null" | "true" | "unknown")
}

fn is_sql_keyword(token: &str) -> bool {
    matches!(
        token,
        "all"
            | "alter"
            | "and"
            | "any"
            | "as"
            | "asc"
            | "begin"
            | "between"
            | "by"
            | "case"
            | "check"
            | "commit"
            | "conflict"
            | "constraint"
            | "create"
            | "cross"
            | "current"
            | "default"
            | "delete"
            | "desc"
            | "distinct"
            | "do"
            | "drop"
            | "else"
            | "end"
            | "except"
            | "exists"
            | "for"
            | "foreign"
            | "from"
            | "full"
            | "function"
            | "grant"
            | "group"
            | "having"
            | "if"
            | "ilike"
            | "in"
            | "index"
            | "inner"
            | "insert"
            | "intersect"
            | "into"
            | "is"
            | "join"
            | "language"
            | "lateral"
            | "left"
            | "limit"
            | "not"
            | "nulls"
            | "offset"
            | "on"
            | "or"
            | "order"
            | "outer"
            | "over"
            | "partition"
            | "primary"
            | "references"
            | "return"
            | "returning"
            | "right"
            | "rollback"
            | "select"
            | "set"
            | "table"
            | "then"
            | "to"
            | "truncate"
            | "union"
            | "unique"
            | "update"
            | "using"
            | "values"
            | "when"
            | "where"
            | "window"
            | "with"
    )
}

fn is_postgres_type(token: &str) -> bool {
    matches!(
        token,
        "bigint"
            | "bigserial"
            | "bit"
            | "boolean"
            | "box"
            | "bytea"
            | "char"
            | "character"
            | "cidr"
            | "circle"
            | "date"
            | "decimal"
            | "double"
            | "inet"
            | "int"
            | "int2"
            | "int4"
            | "int8"
            | "integer"
            | "interval"
            | "json"
            | "jsonb"
            | "line"
            | "lseg"
            | "macaddr"
            | "money"
            | "numeric"
            | "path"
            | "pg_lsn"
            | "plpgsql"
            | "point"
            | "polygon"
            | "real"
            | "serial"
            | "serial2"
            | "serial4"
            | "serial8"
            | "smallint"
            | "smallserial"
            | "text"
            | "time"
            | "timestamp"
            | "timestamptz"
            | "timetz"
            | "trigger"
            | "tsquery"
            | "tsvector"
            | "uuid"
            | "varbit"
            | "varchar"
            | "xml"
    )
}

fn sql_operator_len(text: &str) -> Option<usize> {
    [
        "#>>", "->>", "::", "->", "#>", "@>", "<@", "&&", "||", "!=", "<>", "<=", ">=", ":=", "=>",
    ]
    .iter()
    .find_map(|operator| text.starts_with(operator).then_some(operator.len()))
    .or_else(|| {
        text.chars()
            .next()
            .filter(|ch| {
                matches!(
                    ch,
                    '(' | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | ','
                        | ';'
                        | '.'
                        | ':'
                        | '+'
                        | '-'
                        | '*'
                        | '/'
                        | '%'
                        | '='
                        | '<'
                        | '>'
                        | '!'
                        | '~'
                        | '@'
                        | '#'
                        | '^'
                        | '&'
                        | '|'
                )
            })
            .map(char::len_utf8)
    })
}

fn highlight_http_lines(source: &str) -> Vec<Vec<Segment>> {
    let mut state = HttpState::Start;
    let mut content_type = BodyKind::Unknown;
    let mut out = Vec::new();

    for line in source_lines(source) {
        if is_http_comment_line(line) {
            out.push(vec![Segment::new(line, Style::code_comment())]);
            continue;
        }

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
                if is_http_response_start_line(line) {
                    out.push(highlight_http_start_line(line));
                    state = HttpState::Headers;
                    content_type = BodyKind::Unknown;
                } else {
                    out.push(highlight_body_line(line, kind));
                }
            }
        }
    }

    if out.is_empty() {
        out.push(vec![Segment::new("", Style::code())]);
    }
    out
}

fn is_http_comment_line(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

fn is_http_response_start_line(line: &str) -> bool {
    response_status_parts(line).is_some()
}

fn highlight_http_start_line(line: &str) -> Vec<Segment> {
    if let Some((protocol, status, reason)) = response_status_parts(line) {
        return highlight_http_response_start_line(protocol, status, reason);
    }

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

fn response_status_parts(line: &str) -> Option<(Option<&str>, &str, &str)> {
    let trimmed = line.trim_start();
    let (first, rest) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
    if is_http_status_code(first) {
        return Some((None, first, rest));
    }

    if !first.starts_with("HTTP") {
        return None;
    }

    let rest = rest.trim_start();
    let (status, reason) = rest.split_once(' ').unwrap_or((rest, ""));
    is_http_status_code(status).then_some((Some(first), status, reason))
}

fn is_http_status_code(token: &str) -> bool {
    token.len() == 3
        && token.starts_with(|ch: char| matches!(ch, '1'..='5'))
        && token.chars().all(|ch| ch.is_ascii_digit())
}

fn highlight_http_response_start_line(
    protocol: Option<&str>,
    status: &str,
    reason: &str,
) -> Vec<Segment> {
    let mut segments = Vec::new();

    if let Some(protocol) = protocol {
        push_segment(&mut segments, protocol, Style::code_keyword());
        push_segment(&mut segments, " ", Style::code());
    }

    push_segment(&mut segments, status, Style::code_number());
    if !reason.is_empty() {
        push_segment(&mut segments, " ", Style::code());
        push_segment(&mut segments, reason, Style::code());
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
    fn highlights_http_hash_comments_without_changing_state() {
        let source = "# request note\nPOST /items HTTP/1.1\n# header note\nContent-Type: application/json\n# body follows\n\n# body note\n{\"ok\": true}\n>>>\n# response note\nHTTP 200 OK\nContent-Type: application/json\n\n  # response body note\n{\"id\": 2}";
        let lines = highlight_code("http", source);

        assert_eq!(*find_style(&lines, "# request note"), Style::code_comment());
        assert_eq!(*find_style(&lines, "POST"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "# header note"), Style::code_comment());
        assert_eq!(*find_style(&lines, "Content-Type"), Style::code_key());
        assert_eq!(*find_style(&lines, "# body note"), Style::code_comment());
        assert_eq!(*find_style(&lines, r#""ok""#), Style::code_key());
        assert_eq!(
            *find_style(&lines, "# response note"),
            Style::code_comment()
        );
        assert_eq!(*find_style(&lines, "HTTP"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "200"), Style::code_number());
        assert_eq!(
            *find_style(&lines, "  # response body note"),
            Style::code_comment()
        );
        assert_eq!(*find_style(&lines, r#""id""#), Style::code_key());
    }

    #[test]
    fn highlights_http_bare_status_response_json_without_separator() {
        let source = "GET /endpoint\nAuthorization: ...\n\n200 OK\nContent-Type: application/json\n\n{\"ok\": true}";
        let lines = highlight_code("http", source);

        assert_eq!(*find_style(&lines, "GET"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "Authorization"), Style::code_key());
        assert_eq!(*find_style(&lines, "200"), Style::code_number());
        assert_eq!(*find_style(&lines, "Content-Type"), Style::code_key());
        assert_eq!(*find_style(&lines, r#""ok""#), Style::code_key());
    }

    #[test]
    fn highlights_postgres_sql_tokens() {
        let source = "-- fetch active users\nSELECT u.id, u.name::text, $1::uuid, now()\nFROM \"user\" AS u\nWHERE u.deleted_at IS NULL\n  AND u.profile @> '{\"role\":\"admin\"}'::jsonb\nRETURNING jsonb_build_object('id', u.id);";
        let lines = highlight_code("postgresql", source);

        assert_eq!(
            *find_style(&lines, "-- fetch active users"),
            Style::code_comment()
        );
        assert_eq!(*find_style(&lines, "SELECT"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "text"), Style::code_key());
        assert_eq!(*find_style(&lines, "$1"), Style::code_number());
        assert_eq!(*find_style(&lines, "uuid"), Style::code_key());
        assert_eq!(*find_style(&lines, "now"), Style::code_key());
        assert_eq!(*find_style(&lines, r#""user""#), Style::code_key());
        assert_eq!(*find_style(&lines, "NULL"), Style::code_literal());
        assert_eq!(*find_style(&lines, "@>"), Style::code_punctuation());
        assert_eq!(
            *find_style(&lines, r#"'{"role":"admin"}'"#),
            Style::code_string()
        );
        assert_eq!(*find_style(&lines, "jsonb"), Style::code_key());
        assert_eq!(*find_style(&lines, "RETURNING"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "jsonb_build_object"), Style::code_key());
        assert_eq!(*find_style(&lines, "'id'"), Style::code_string());
    }

    #[test]
    fn highlights_postgres_dollar_strings_and_block_comments() {
        let source = "/* outer\n   /* nested */ comment */\nCREATE FUNCTION touch_user() RETURNS trigger AS $$\nBEGIN\n  NEW.updated_at := now();\n  RETURN NEW;\nEND\n$$ LANGUAGE plpgsql;";
        let lines = highlight_code("sql", source);

        assert_eq!(*find_style(&lines, "/* outer"), Style::code_comment());
        assert_eq!(
            *find_style(&lines, "   /* nested */ comment */"),
            Style::code_comment()
        );
        assert_eq!(*find_style(&lines, "CREATE"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "touch_user"), Style::code_key());
        assert_eq!(*find_style(&lines, "$$"), Style::code_string());
        assert_eq!(*find_style(&lines, "BEGIN"), Style::code_string());
        assert_eq!(*find_style(&lines, "LANGUAGE"), Style::code_keyword());
        assert_eq!(*find_style(&lines, "plpgsql"), Style::code_key());
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
