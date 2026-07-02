use crate::rendered::ImageSlot;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageMode {
    Direct,
    TmuxPassthrough,
    Disabled(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
}

pub fn detect_image_mode() -> ImageMode {
    let env_get = |key: &str| env::var(key).ok();
    let tmux_allow = tmux_allow_passthrough();
    detect_image_mode_from(env_get, tmux_allow.as_deref())
}

pub fn detect_image_mode_from<F>(env_get: F, _tmux_allow_passthrough: Option<&str>) -> ImageMode
where
    F: Fn(&str) -> Option<String>,
{
    let override_mode = env_get("MDVIEW_IMAGES")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        override_mode.as_str(),
        "0" | "off" | "false" | "no" | "never"
    ) {
        return ImageMode::Disabled("image output disabled by MDVIEW_IMAGES".to_string());
    }

    let term_program = env_get("TERM_PROGRAM").unwrap_or_default();
    let term = env_get("TERM").unwrap_or_default();
    let iterm_session = env_get("ITERM_SESSION_ID").is_some();
    let lc_terminal = env_get("LC_TERMINAL").unwrap_or_default();
    let is_iterm = term_program == "iTerm.app" || lc_terminal == "iTerm2" || iterm_session;
    let inside_tmux = env_get("TMUX").is_some()
        || term_program == "tmux"
        || term.starts_with("tmux")
        || term.starts_with("screen");

    if matches!(
        override_mode.as_str(),
        "1" | "on" | "true" | "yes" | "always" | "force" | "iterm2"
    ) {
        return if inside_tmux {
            ImageMode::TmuxPassthrough
        } else {
            ImageMode::Direct
        };
    }

    if inside_tmux {
        return ImageMode::TmuxPassthrough;
    }

    if !is_iterm {
        return ImageMode::Disabled(
            "not running inside iTerm2; set MDVIEW_IMAGES=force to override".to_string(),
        );
    }

    ImageMode::Direct
}

pub fn tmux_allow_passthrough() -> Option<String> {
    let output = Command::new("tmux")
        .args(["show-options", "-gqv", "allow-passthrough"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;

        out.push(BASE64[((triple >> 18) & 0x3f) as usize] as char);
        out.push(BASE64[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn image_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    png_dimensions(bytes)
        .or_else(|| gif_dimensions(bytes))
        .or_else(|| jpeg_dimensions(bytes))
        .or_else(|| svg_dimensions(bytes))
        .or_else(|| ppm_dimensions(bytes))
}

pub fn build_image_slot(path: PathBuf, alt: String, max_width: u16) -> io::Result<ImageSlot> {
    let bytes = fs::read(&path)?;
    let dimensions = image_dimensions(&bytes);
    let (width_cells, height_cells) = dimensions
        .map(|dims| scaled_cells(dims, max_width))
        .unwrap_or_else(|| (max_width.clamp(8, 48), 6));

    Ok(ImageSlot {
        path,
        alt,
        width_cells,
        height_cells,
        original_width: dimensions.map(|dims| dims.width),
        original_height: dimensions.map(|dims| dims.height),
    })
}

pub fn iterm2_image_sequence(
    data: &[u8],
    name: &str,
    width_cells: u16,
    height_cells: u16,
) -> String {
    let size = data.len();
    let name = base64_encode(name.as_bytes());
    let data = base64_encode(data);
    format!(
        "\x1b]1337;File=name={name};size={};inline=1;width={};height={};preserveAspectRatio=1:{data}\x07",
        size,
        width_cells.max(1),
        height_cells.max(1)
    )
}

pub fn tmux_passthrough(sequence: &str) -> String {
    let mut out = String::from("\x1bPtmux;");
    for ch in sequence.chars() {
        if ch == '\x1b' {
            out.push('\x1b');
            out.push('\x1b');
        } else {
            out.push(ch);
        }
    }
    out.push_str("\x1b\\");
    out
}

pub fn resolve_image_path(markdown_file: &Path, dest: &str) -> Option<PathBuf> {
    if dest.contains("://") || dest.starts_with("data:") || dest.starts_with('#') {
        return None;
    }
    let dest_path = Path::new(dest);
    if dest_path.is_absolute() {
        return Some(dest_path.to_path_buf());
    }
    Some(
        markdown_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(dest_path),
    )
}

fn png_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    let signature = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != signature || &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some(ImageDimensions {
        width: u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        height: u32::from_be_bytes(bytes[20..24].try_into().ok()?),
    })
}

fn gif_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 10 || !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return None;
    }
    Some(ImageDimensions {
        width: u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
        height: u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
    })
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }

    let mut i = 2;
    while i + 3 < bytes.len() {
        while i < bytes.len() && bytes[i] != 0xff {
            i += 1;
        }
        while i < bytes.len() && bytes[i] == 0xff {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let marker = bytes[i];
        i += 1;

        if matches!(marker, 0x01 | 0xd0..=0xd9) {
            continue;
        }
        if i + 1 >= bytes.len() {
            return None;
        }
        let len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
        i += 2;
        if len < 2 || i + len - 2 > bytes.len() {
            return None;
        }

        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if len < 7 {
                return None;
            }
            let height = u16::from_be_bytes([bytes[i + 1], bytes[i + 2]]) as u32;
            let width = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]) as u32;
            return Some(ImageDimensions { width, height });
        }

        i += len - 2;
    }

    None
}

fn svg_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    let text = std::str::from_utf8(bytes.get(..bytes.len().min(4096))?).ok()?;
    if !text.contains("<svg") {
        return None;
    }

    let width = svg_attr_number(text, "width");
    let height = svg_attr_number(text, "height");
    if let (Some(width), Some(height)) = (width, height) {
        return Some(ImageDimensions { width, height });
    }

    let view_box = svg_attr_value(text, "viewBox").or_else(|| svg_attr_value(text, "viewbox"))?;
    let nums: Vec<u32> = view_box
        .split(|ch: char| ch.is_ascii_whitespace() || ch == ',')
        .filter_map(parse_u32_prefix)
        .collect();
    if nums.len() == 4 && nums[2] > 0 && nums[3] > 0 {
        return Some(ImageDimensions {
            width: nums[2],
            height: nums[3],
        });
    }

    None
}

fn ppm_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    let text = std::str::from_utf8(bytes.get(..bytes.len().min(256))?).ok()?;
    let mut tokens = Vec::new();
    for line in text.lines() {
        let line = line.split_once('#').map_or(line, |(head, _)| head);
        tokens.extend(line.split_whitespace());
        if tokens.len() >= 3 {
            break;
        }
    }
    if !matches!(tokens.first(), Some(&"P3" | &"P6")) {
        return None;
    }
    Some(ImageDimensions {
        width: tokens.get(1)?.parse().ok()?,
        height: tokens.get(2)?.parse().ok()?,
    })
}

fn svg_attr_number(text: &str, attr: &str) -> Option<u32> {
    svg_attr_value(text, attr).and_then(|value| parse_u32_prefix(&value))
}

fn svg_attr_value(text: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=");
    let idx = text.find(&needle)? + needle.len();
    let quote = text.as_bytes().get(idx).copied()?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let rest = &text[idx + 1..];
    let end = rest.find(quote as char)?;
    Some(rest[..end].to_string())
}

fn parse_u32_prefix(value: &str) -> Option<u32> {
    let mut digits = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if ch == '.' {
            break;
        } else if digits.is_empty() && (ch == '+' || ch == '-') {
            return None;
        } else {
            break;
        }
    }
    let parsed: u32 = digits.parse().ok()?;
    (parsed > 0).then_some(parsed)
}

fn scaled_cells(dimensions: ImageDimensions, max_width: u16) -> (u16, u16) {
    let max_width = max_width.max(1);
    let width_cells = max_width.clamp(1, 80);
    if dimensions.width == 0 || dimensions.height == 0 {
        return (width_cells, 6);
    }

    let ratio = dimensions.height as f64 / dimensions.width as f64;
    let height_cells = ((width_cells as f64 * ratio) / 2.0).ceil().clamp(1.0, 24.0) as u16;
    (width_cells, height_cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn encodes_base64_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn reads_png_dimensions() {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend_from_slice(&320u32.to_be_bytes());
        bytes.extend_from_slice(&200u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 2, 0, 0, 0]);
        assert_eq!(
            image_dimensions(&bytes),
            Some(ImageDimensions {
                width: 320,
                height: 200
            })
        );
    }

    #[test]
    fn reads_gif_dimensions() {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(&9u16.to_le_bytes());
        assert_eq!(
            image_dimensions(&bytes),
            Some(ImageDimensions {
                width: 16,
                height: 9
            })
        );
    }

    #[test]
    fn reads_jpeg_dimensions() {
        let bytes = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00,
            0x78, 0x00, 0xa0, 0x03, 0x01, 0x11, 0x00,
        ];
        assert_eq!(
            image_dimensions(&bytes),
            Some(ImageDimensions {
                width: 160,
                height: 120
            })
        );
    }

    #[test]
    fn reads_svg_and_ppm_dimensions() {
        assert_eq!(
            image_dimensions(br#"<svg width="120px" height="80"></svg>"#),
            Some(ImageDimensions {
                width: 120,
                height: 80
            })
        );
        assert_eq!(
            image_dimensions(b"P3\n# comment\n4 3\n255\n"),
            Some(ImageDimensions {
                width: 4,
                height: 3
            })
        );
    }

    #[test]
    fn builds_iterm2_sequence_and_tmux_wrapper() {
        let sequence = iterm2_image_sequence(b"abc", "a.png", 10, 3);
        assert!(sequence.starts_with("\x1b]1337;File=name=YS5wbmc=;"));
        assert!(sequence.contains(";inline=1;width=10;height=3;"));
        assert!(sequence.ends_with("YWJj\x07"));

        let wrapped = tmux_passthrough(&sequence);
        assert!(wrapped.starts_with("\x1bPtmux;\x1b\x1b]1337;"));
        assert!(wrapped.ends_with("\x07\x1b\\"));
    }

    #[test]
    fn detects_image_modes() {
        let mut values = HashMap::new();
        values.insert("TERM_PROGRAM", "iTerm.app");
        let env_get = |key: &str| values.get(key).map(|value| value.to_string());
        assert_eq!(detect_image_mode_from(env_get, None), ImageMode::Direct);

        values.clear();
        values.insert("TERM", "xterm-256color");
        let env_get = |key: &str| values.get(key).map(|value| value.to_string());
        assert!(matches!(
            detect_image_mode_from(env_get, None),
            ImageMode::Disabled(reason) if reason.contains("MDVIEW_IMAGES=force")
        ));

        values.clear();
        values.insert("TERM_PROGRAM", "tmux");
        values.insert("TMUX", "/tmp/tmux");
        let env_get = |key: &str| values.get(key).map(|value| value.to_string());
        assert_eq!(
            detect_image_mode_from(env_get, Some("off")),
            ImageMode::TmuxPassthrough
        );

        values.clear();
        values.insert("TERM", "screen-256color");
        let env_get = |key: &str| values.get(key).map(|value| value.to_string());
        assert_eq!(
            detect_image_mode_from(env_get, None),
            ImageMode::TmuxPassthrough
        );

        values.clear();
        values.insert("MDVIEW_IMAGES", "off");
        values.insert("TERM_PROGRAM", "iTerm.app");
        let env_get = |key: &str| values.get(key).map(|value| value.to_string());
        assert!(matches!(
            detect_image_mode_from(env_get, None),
            ImageMode::Disabled(reason) if reason.contains("disabled")
        ));

        values.clear();
        values.insert("MDVIEW_IMAGES", "force");
        let env_get = |key: &str| values.get(key).map(|value| value.to_string());
        assert_eq!(detect_image_mode_from(env_get, None), ImageMode::Direct);

        values.clear();
        values.insert("TMUX", "/tmp/tmux");
        values.insert("TERM_PROGRAM", "tmux");
        values.insert("ITERM_SESSION_ID", "w0t0p0");
        let env_get = |key: &str| values.get(key).map(|value| value.to_string());
        assert_eq!(
            detect_image_mode_from(env_get, Some("on")),
            ImageMode::TmuxPassthrough
        );
        assert_eq!(
            detect_image_mode_from(env_get, Some("off")),
            ImageMode::TmuxPassthrough
        );
    }

    #[test]
    fn resolves_only_local_image_paths() {
        let md = Path::new("/tmp/docs/readme.md");
        assert_eq!(
            resolve_image_path(md, "assets/a.png").unwrap(),
            PathBuf::from("/tmp/docs/assets/a.png")
        );
        assert!(resolve_image_path(md, "https://example.com/a.png").is_none());
        assert!(resolve_image_path(md, "data:image/png;base64,abc").is_none());
    }
}
