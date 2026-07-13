//! Terminal cell-width helpers.
//!
//! Scalar widths use complete Unicode 17.0 zero-width and East Asian
//! wide/fullwidth tables. String widths additionally account for terminal
//! presentation sequences whose width is not the sum of their scalar widths.

pub fn str_width(text: &str) -> usize {
    width_chars(text).map(|(_, width)| width).sum()
}

pub fn width_chars(text: &str) -> impl Iterator<Item = (char, usize)> {
    let chars = text.chars().collect::<Vec<_>>();
    let kirat_widths = kirat_rai_width_overrides(&chars);
    let mut state = WidthState::default();
    chars
        .iter()
        .enumerate()
        .map(|(index, &ch)| {
            let next = chars.get(index + 1).copied();
            let next_script_character = chars[index + 1..]
                .iter()
                .copied()
                .find(|candidate| *candidate != '\u{200d}' && !is_ligature_transparent(*candidate));
            let width_override = if next == Some('\u{fe01}')
                && matches!(ch, '\u{2018}' | '\u{2019}' | '\u{201c}' | '\u{201d}')
            {
                Some(2)
            } else if next == Some('\u{fe0e}')
                && starts_non_ideographic_text_presentation_sequence(ch)
            {
                Some(1)
            } else if ch == '\u{2d7f}'
                && state.can_start_tifinagh_joiner()
                && next_script_character.is_some_and(is_tifinagh_consonant)
            {
                Some(0)
            } else {
                kirat_widths[index]
            };
            (ch, state.push_with_width(ch, width_override, next))
        })
        .collect::<Vec<_>>()
        .into_iter()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KiratRaiSuffix {
    None,
    VowelE,
    VowelAi,
}

fn kirat_rai_width_overrides(chars: &[char]) -> Vec<Option<usize>> {
    let mut overrides = vec![None; chars.len()];
    let mut suffix = KiratRaiSuffix::None;
    for (index, &ch) in chars.iter().enumerate().rev() {
        suffix = match (suffix, ch) {
            (KiratRaiSuffix::VowelE, '\u{16d63}') => {
                overrides[index] = Some(0);
                KiratRaiSuffix::None
            }
            (KiratRaiSuffix::VowelE, '\u{16d67}') => {
                overrides[index] = Some(0);
                KiratRaiSuffix::VowelAi
            }
            (KiratRaiSuffix::VowelE, '\u{16d68}') => KiratRaiSuffix::VowelE,
            (KiratRaiSuffix::VowelE, '\u{16d69}') => {
                overrides[index] = Some(0);
                KiratRaiSuffix::None
            }
            (KiratRaiSuffix::VowelAi, '\u{16d63}') => {
                overrides[index] = Some(0);
                KiratRaiSuffix::None
            }
            (_, '\u{16d67}') => KiratRaiSuffix::VowelE,
            (_, '\u{16d68}') => KiratRaiSuffix::VowelAi,
            _ => KiratRaiSuffix::None,
        };
    }
    overrides
}

#[derive(Debug, Default)]
struct WidthState {
    display_width: usize,
    cluster_width: usize,
    base: Option<char>,
    zwj_count: u8,
    zwj_left_flag_complete: bool,
    khmer_coeng: bool,
    tifinagh_joiner: bool,
    regional_indicators: u8,
    script_transparent: bool,
    buginese_vowels: u8,
    emoji_tag_count: u8,
    emoji_tag_blocked: bool,
    variation_eligible: bool,
    emoji_presentation: bool,
    emoji_modifier_eligible: bool,
    arabic_ligature_blocked: bool,
}

impl WidthState {
    fn push_with_width(
        &mut self,
        ch: char,
        width_override: Option<usize>,
        next: Option<char>,
    ) -> usize {
        let before = self.display_width;
        let code = ch as u32;
        let text_presentation = width_override == Some(1);
        let Some(width) = width_override.or_else(|| char_width(ch)) else {
            self.cluster_width = 0;
            self.base = None;
            self.zwj_count = 0;
            self.zwj_left_flag_complete = false;
            self.khmer_coeng = false;
            self.tifinagh_joiner = false;
            self.regional_indicators = 0;
            self.script_transparent = false;
            self.buginese_vowels = 0;
            self.emoji_tag_count = 0;
            self.emoji_tag_blocked = false;
            self.variation_eligible = false;
            self.emoji_presentation = false;
            self.emoji_modifier_eligible = false;
            self.arabic_ligature_blocked = false;
            return 0;
        };

        if ch == '\u{200d}' {
            if self.zwj_count == 0 {
                self.zwj_left_flag_complete = self.regional_indicators == 2;
            }
            self.zwj_count = self.zwj_count.saturating_add(1);
            self.regional_indicators = 0;
            self.variation_eligible = false;
            self.emoji_modifier_eligible = false;
            self.arabic_ligature_blocked = true;
            return 0;
        }

        if ch == '\u{2d7f}' && width == 0 && self.base.is_some_and(is_tifinagh_consonant) {
            self.tifinagh_joiner = true;
            self.variation_eligible = false;
            self.emoji_modifier_eligible = false;
            return 0;
        }

        if ch == '\u{fe0f}' {
            let promotes = self.variation_eligible
                && self.base.is_some_and(starts_emoji_presentation_sequence);
            if promotes {
                self.grow_cluster_to(2);
            }
            self.emoji_presentation = promotes;
            self.variation_eligible = false;
            self.emoji_modifier_eligible = false;
            return self.display_width.saturating_sub(before);
        }

        if ch == '\u{fe0e}' {
            self.emoji_presentation = false;
            self.variation_eligible = false;
            self.emoji_modifier_eligible = false;
            return 0;
        }

        if matches!(ch, '\u{fe00}'..='\u{fe02}') {
            self.emoji_presentation = false;
            self.variation_eligible = false;
            self.emoji_modifier_eligible = false;
            return 0;
        }

        if ch == '\u{20e3}' {
            if self
                .base
                .is_some_and(|base| matches!(base, '#' | '*' | '0'..='9'))
            {
                self.grow_cluster_to(2);
            }
            self.emoji_presentation = false;
            self.variation_eligible = false;
            self.emoji_modifier_eligible = false;
            return self.display_width.saturating_sub(before);
        }

        if is_emoji_tag(code) {
            if ch == '\u{e007f}' {
                self.emoji_tag_blocked =
                    !(3..=6).contains(&self.emoji_tag_count) || self.base != Some('\u{1f3f4}');
                if !self.emoji_tag_blocked {
                    self.emoji_presentation = true;
                }
            } else if is_emoji_tag_alphanumeric(code) {
                self.emoji_tag_count = self.emoji_tag_count.saturating_add(1);
                self.emoji_tag_blocked = true;
            } else {
                self.emoji_tag_blocked = true;
            }
            self.zwj_count = 0;
            self.script_transparent = false;
            self.tifinagh_joiner = false;
            self.variation_eligible = false;
            self.emoji_modifier_eligible = false;
            return 0;
        }

        if width == 0 {
            if ch == '\u{17d2}' {
                self.khmer_coeng = self.base.is_none_or(is_khmer_coeng_base);
            } else if ch == '\u{1a17}' && self.base == Some('\u{1a15}') && self.zwj_count == 0 {
                self.buginese_vowels = self.buginese_vowels.saturating_add(1);
            } else if !is_ligature_transparent(ch) {
                self.script_transparent = false;
            }
            self.variation_eligible = false;
            self.emoji_modifier_eligible = false;
            return 0;
        }

        if is_emoji_modifier(code) && self.zwj_count == 0 && self.emoji_modifier_eligible {
            self.grow_cluster_to(2);
            self.emoji_presentation = true;
            self.variation_eligible = false;
            self.emoji_modifier_eligible = false;
            return self.display_width.saturating_sub(before);
        }

        let joins_emoji = self.joins_emoji_with_zwj(ch, next);
        let joins_script = self.joins_script_with_zwj(ch);
        let joins_previous = joins_emoji
            || joins_script
            || self.forms_unjoined_ligature(ch)
            || (self.khmer_coeng && is_khmer_coeng_base(ch))
            || (self.tifinagh_joiner && is_tifinagh_consonant(ch));
        let regional_indicator = is_regional_indicator(ch);
        let completes_regional_pair = regional_indicator && self.regional_indicators == 1;

        if completes_regional_pair {
            self.grow_cluster_to(2);
        } else if joins_previous {
            if !(self.khmer_coeng && self.cluster_width == 0) {
                self.grow_cluster_to(width);
            }
        } else {
            self.cluster_width = width;
            self.display_width = self.display_width.saturating_add(width);
        }

        self.base = Some(ch);
        self.zwj_count = 0;
        self.zwj_left_flag_complete = false;
        self.khmer_coeng = false;
        self.tifinagh_joiner = false;
        self.regional_indicators = if regional_indicator {
            if completes_regional_pair {
                2
            } else {
                1
            }
        } else {
            0
        };
        self.script_transparent = true;
        self.buginese_vowels = 0;
        self.emoji_tag_count = 0;
        self.emoji_tag_blocked = false;
        self.variation_eligible = true;
        self.emoji_modifier_eligible = is_emoji_modifier_base(ch);
        self.emoji_presentation = if completes_regional_pair || joins_emoji {
            true
        } else if joins_previous {
            false
        } else {
            !text_presentation && is_default_emoji_presentation(ch)
        };
        self.arabic_ligature_blocked = false;
        self.display_width.saturating_sub(before)
    }

    fn grow_cluster_to(&mut self, width: usize) {
        if width > self.cluster_width {
            self.display_width = self
                .display_width
                .saturating_add(width - self.cluster_width);
            self.cluster_width = width;
        }
    }

    fn can_start_tifinagh_joiner(&self) -> bool {
        self.base.is_some_and(is_tifinagh_consonant) && self.script_transparent
    }

    fn joins_emoji_with_zwj(&self, right: char, next: Option<char>) -> bool {
        let Some(left) = self.base else {
            return false;
        };
        if self.zwj_count != 1
            || !self.script_transparent
            || !self.emoji_presentation
            || self.emoji_tag_blocked
        {
            return false;
        }
        if is_regional_indicator(right) {
            return next.is_some_and(is_regional_indicator)
                && (!is_regional_indicator(left) || self.zwj_left_flag_complete);
        }
        (is_default_emoji_presentation(right)
            && !(next == Some('\u{fe0e}')
                && starts_non_ideographic_text_presentation_sequence(right)))
            || (next == Some('\u{fe0f}') && starts_emoji_presentation_sequence(right))
            || is_emoji_modifier(right as u32)
    }

    fn joins_script_with_zwj(&self, right: char) -> bool {
        let Some(left) = self.base else {
            return false;
        };
        if self.zwj_count == 0 || !self.script_transparent {
            return false;
        }
        matches!(
            (left, right),
            ('\u{05d0}', '\u{05dc}') | ('\u{10c32}', '\u{10c03}')
        ) || (is_tifinagh_consonant(left) && is_tifinagh_consonant(right))
            || (left == '\u{1a15}' && right == '\u{1a10}' && self.buginese_vowels == 1)
    }

    fn forms_unjoined_ligature(&self, right: char) -> bool {
        let Some(left) = self.base else {
            return false;
        };
        (!self.arabic_ligature_blocked && is_arabic_lam(left) && is_arabic_alef(right))
            || (self.script_transparent
                && matches!(left as u32, 0xa4f8..=0xa4fb)
                && matches!(right as u32, 0xa4fc..=0xa4fd))
    }
}

pub fn char_width(ch: char) -> Option<usize> {
    let code = ch as u32;
    if code == 0 {
        return Some(0);
    }
    if ch.is_control() {
        return None;
    }
    if code == 0x17d8 {
        return Some(3);
    }
    if in_ranges(code, ZERO_WIDTH_RANGES) {
        return Some(0);
    }
    Some(if in_ranges(code, WIDE_RANGES) { 2 } else { 1 })
}

fn is_arabic_lam(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0644 | 0x06b5..=0x06b8 | 0x076a | 0x08a6 | 0x08c7
    )
}

fn is_arabic_alef(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0622..=0x0625 | 0x0627 | 0x0671..=0x0673 | 0x0675 | 0x0773..=0x0774
    )
}

fn is_khmer_coeng_base(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1780..=0x1782
            | 0x1784..=0x1787
            | 0x1789..=0x178c
            | 0x178e..=0x1793
            | 0x1795..=0x1798
            | 0x179b..=0x179d
            | 0x17a0
            | 0x17a2
            | 0x17a7
            | 0x17ab..=0x17ac
            | 0x17af
    )
}

fn is_tifinagh_consonant(ch: char) -> bool {
    matches!(ch as u32, 0x2d31..=0x2d65 | 0x2d6f)
}

fn is_regional_indicator(ch: char) -> bool {
    matches!(ch as u32, 0x1f1e6..=0x1f1ff)
}

fn is_ligature_transparent(ch: char) -> bool {
    matches!(
        ch as u32,
        0x034f
            | 0x17b4..=0x17b5
            | 0x180b..=0x180d
            | 0x180f
            | 0x200d
            | 0xfe00..=0xfe0f
            | 0xe0100..=0xe01ef
    )
}

fn is_emoji_tag(code: u32) -> bool {
    matches!(code, 0xe0020..=0xe007f)
}

fn is_emoji_tag_alphanumeric(code: u32) -> bool {
    matches!(code, 0xe0030..=0xe0039 | 0xe0061..=0xe007a)
}

fn is_emoji_modifier(code: u32) -> bool {
    matches!(code, 0x1f3fb..=0x1f3ff)
}

fn is_emoji_modifier_base(ch: char) -> bool {
    in_ranges(ch as u32, EMOJI_MODIFIER_BASE_RANGES)
}

fn is_default_emoji_presentation(ch: char) -> bool {
    in_ranges(ch as u32, DEFAULT_EMOJI_PRESENTATION_RANGES)
}

fn starts_emoji_presentation_sequence(ch: char) -> bool {
    in_ranges(ch as u32, EMOJI_VARIATION_BASE_RANGES)
        || in_ranges(ch as u32, TEXT_PRESENTATION_BASE_RANGES)
}

fn starts_non_ideographic_text_presentation_sequence(ch: char) -> bool {
    in_ranges(ch as u32, TEXT_PRESENTATION_BASE_RANGES)
}

fn in_ranges(code: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .binary_search_by(|&(start, end)| {
            if end < code {
                std::cmp::Ordering::Less
            } else if start > code {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

const DEFAULT_EMOJI_PRESENTATION_RANGES: &[(u32, u32)] = &[
    (0x231a, 0x231b),
    (0x23e9, 0x23ec),
    (0x23f0, 0x23f0),
    (0x23f3, 0x23f3),
    (0x25fd, 0x25fe),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267f, 0x267f),
    (0x2693, 0x2693),
    (0x26a1, 0x26a1),
    (0x26aa, 0x26ab),
    (0x26bd, 0x26be),
    (0x26c4, 0x26c5),
    (0x26ce, 0x26ce),
    (0x26d4, 0x26d4),
    (0x26ea, 0x26ea),
    (0x26f2, 0x26f3),
    (0x26f5, 0x26f5),
    (0x26fa, 0x26fa),
    (0x26fd, 0x26fd),
    (0x2705, 0x2705),
    (0x270a, 0x270b),
    (0x2728, 0x2728),
    (0x274c, 0x274c),
    (0x274e, 0x274e),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27b0, 0x27b0),
    (0x27bf, 0x27bf),
    (0x2b1b, 0x2b1c),
    (0x2b50, 0x2b50),
    (0x2b55, 0x2b55),
    (0x1f004, 0x1f004),
    (0x1f0cf, 0x1f0cf),
    (0x1f18e, 0x1f18e),
    (0x1f191, 0x1f19a),
    (0x1f201, 0x1f201),
    (0x1f21a, 0x1f21a),
    (0x1f22f, 0x1f22f),
    (0x1f232, 0x1f236),
    (0x1f238, 0x1f23a),
    (0x1f250, 0x1f251),
    (0x1f300, 0x1f320),
    (0x1f32d, 0x1f335),
    (0x1f337, 0x1f37c),
    (0x1f37e, 0x1f393),
    (0x1f3a0, 0x1f3ca),
    (0x1f3cf, 0x1f3d3),
    (0x1f3e0, 0x1f3f0),
    (0x1f3f4, 0x1f3f4),
    (0x1f3f8, 0x1f43e),
    (0x1f440, 0x1f440),
    (0x1f442, 0x1f4fc),
    (0x1f4ff, 0x1f53d),
    (0x1f54b, 0x1f54e),
    (0x1f550, 0x1f567),
    (0x1f57a, 0x1f57a),
    (0x1f595, 0x1f596),
    (0x1f5a4, 0x1f5a4),
    (0x1f5fb, 0x1f64f),
    (0x1f680, 0x1f6c5),
    (0x1f6cc, 0x1f6cc),
    (0x1f6d0, 0x1f6d2),
    (0x1f6d5, 0x1f6d8),
    (0x1f6dc, 0x1f6df),
    (0x1f6eb, 0x1f6ec),
    (0x1f6f4, 0x1f6fc),
    (0x1f7e0, 0x1f7eb),
    (0x1f7f0, 0x1f7f0),
    (0x1f90c, 0x1f93a),
    (0x1f93c, 0x1f945),
    (0x1f947, 0x1f9ff),
    (0x1fa70, 0x1fa7c),
    (0x1fa80, 0x1fa8a),
    (0x1fa8e, 0x1fac6),
    (0x1fac8, 0x1fac8),
    (0x1facd, 0x1fadc),
    (0x1fadf, 0x1faea),
    (0x1faef, 0x1faf8),
];
const EMOJI_MODIFIER_BASE_RANGES: &[(u32, u32)] = &[
    (0x261d, 0x261d),
    (0x26f9, 0x26f9),
    (0x270a, 0x270d),
    (0x1f385, 0x1f385),
    (0x1f3c2, 0x1f3c4),
    (0x1f3c7, 0x1f3c7),
    (0x1f3ca, 0x1f3cc),
    (0x1f442, 0x1f443),
    (0x1f446, 0x1f450),
    (0x1f466, 0x1f478),
    (0x1f47c, 0x1f47c),
    (0x1f481, 0x1f483),
    (0x1f485, 0x1f487),
    (0x1f48f, 0x1f48f),
    (0x1f491, 0x1f491),
    (0x1f4aa, 0x1f4aa),
    (0x1f574, 0x1f575),
    (0x1f57a, 0x1f57a),
    (0x1f590, 0x1f590),
    (0x1f595, 0x1f596),
    (0x1f645, 0x1f647),
    (0x1f64b, 0x1f64f),
    (0x1f6a3, 0x1f6a3),
    (0x1f6b4, 0x1f6b6),
    (0x1f6c0, 0x1f6c0),
    (0x1f6cc, 0x1f6cc),
    (0x1f90c, 0x1f90c),
    (0x1f90f, 0x1f90f),
    (0x1f918, 0x1f91f),
    (0x1f926, 0x1f926),
    (0x1f930, 0x1f939),
    (0x1f93c, 0x1f93e),
    (0x1f977, 0x1f977),
    (0x1f9b5, 0x1f9b6),
    (0x1f9b8, 0x1f9b9),
    (0x1f9bb, 0x1f9bb),
    (0x1f9cd, 0x1f9cf),
    (0x1f9d1, 0x1f9dd),
    (0x1fac3, 0x1fac5),
    (0x1faf0, 0x1faf8),
];
const TEXT_PRESENTATION_BASE_RANGES: &[(u32, u32)] = &[
    (0x231a, 0x231b),
    (0x23e9, 0x23ec),
    (0x23f0, 0x23f0),
    (0x23f3, 0x23f3),
    (0x25fd, 0x25fe),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267f, 0x267f),
    (0x2693, 0x2693),
    (0x26a1, 0x26a1),
    (0x26aa, 0x26ab),
    (0x26bd, 0x26be),
    (0x26c4, 0x26c5),
    (0x26ce, 0x26ce),
    (0x26d4, 0x26d4),
    (0x26ea, 0x26ea),
    (0x26f2, 0x26f3),
    (0x26f5, 0x26f5),
    (0x26fa, 0x26fa),
    (0x26fd, 0x26fd),
    (0x2705, 0x2705),
    (0x270a, 0x270b),
    (0x2728, 0x2728),
    (0x274c, 0x274c),
    (0x274e, 0x274e),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27b0, 0x27b0),
    (0x27bf, 0x27bf),
    (0x2b1b, 0x2b1c),
    (0x2b50, 0x2b50),
    (0x2b55, 0x2b55),
    (0x1f004, 0x1f004),
    (0x1f30d, 0x1f30f),
    (0x1f315, 0x1f315),
    (0x1f31c, 0x1f31c),
    (0x1f378, 0x1f378),
    (0x1f393, 0x1f393),
    (0x1f3a7, 0x1f3a7),
    (0x1f3ac, 0x1f3ae),
    (0x1f3c2, 0x1f3c2),
    (0x1f3c4, 0x1f3c4),
    (0x1f3c6, 0x1f3c6),
    (0x1f3ca, 0x1f3ca),
    (0x1f3e0, 0x1f3e0),
    (0x1f3ed, 0x1f3ed),
    (0x1f408, 0x1f408),
    (0x1f415, 0x1f415),
    (0x1f41f, 0x1f41f),
    (0x1f426, 0x1f426),
    (0x1f442, 0x1f442),
    (0x1f446, 0x1f449),
    (0x1f44d, 0x1f44e),
    (0x1f453, 0x1f453),
    (0x1f46a, 0x1f46a),
    (0x1f47d, 0x1f47d),
    (0x1f4a3, 0x1f4a3),
    (0x1f4b0, 0x1f4b0),
    (0x1f4b3, 0x1f4b3),
    (0x1f4bb, 0x1f4bb),
    (0x1f4bf, 0x1f4bf),
    (0x1f4cb, 0x1f4cb),
    (0x1f4da, 0x1f4da),
    (0x1f4df, 0x1f4df),
    (0x1f4e4, 0x1f4e6),
    (0x1f4ea, 0x1f4ed),
    (0x1f4f7, 0x1f4f7),
    (0x1f4f9, 0x1f4fb),
    (0x1f508, 0x1f508),
    (0x1f50d, 0x1f50d),
    (0x1f512, 0x1f513),
    (0x1f550, 0x1f567),
    (0x1f610, 0x1f610),
    (0x1f687, 0x1f687),
    (0x1f68d, 0x1f68d),
    (0x1f691, 0x1f691),
    (0x1f694, 0x1f694),
    (0x1f698, 0x1f698),
    (0x1f6ad, 0x1f6ad),
    (0x1f6b2, 0x1f6b2),
    (0x1f6b9, 0x1f6ba),
    (0x1f6bc, 0x1f6bc),
];

const EMOJI_VARIATION_BASE_RANGES: &[(u32, u32)] = &[
    (0x23, 0x23),
    (0x2a, 0x2a),
    (0x30, 0x39),
    (0xa9, 0xa9),
    (0xae, 0xae),
    (0x203c, 0x203c),
    (0x2049, 0x2049),
    (0x2122, 0x2122),
    (0x2139, 0x2139),
    (0x2194, 0x2199),
    (0x21a9, 0x21aa),
    (0x2328, 0x2328),
    (0x23cf, 0x23cf),
    (0x23ed, 0x23ef),
    (0x23f1, 0x23f2),
    (0x23f8, 0x23fa),
    (0x24c2, 0x24c2),
    (0x25aa, 0x25ab),
    (0x25b6, 0x25b6),
    (0x25c0, 0x25c0),
    (0x25fb, 0x25fc),
    (0x2600, 0x2604),
    (0x260e, 0x260e),
    (0x2611, 0x2611),
    (0x2618, 0x2618),
    (0x261d, 0x261d),
    (0x2620, 0x2620),
    (0x2622, 0x2623),
    (0x2626, 0x2626),
    (0x262a, 0x262a),
    (0x262e, 0x262f),
    (0x2638, 0x263a),
    (0x2640, 0x2640),
    (0x2642, 0x2642),
    (0x265f, 0x2660),
    (0x2663, 0x2663),
    (0x2665, 0x2666),
    (0x2668, 0x2668),
    (0x267b, 0x267b),
    (0x267e, 0x267e),
    (0x2692, 0x2692),
    (0x2694, 0x2697),
    (0x2699, 0x2699),
    (0x269b, 0x269c),
    (0x26a0, 0x26a0),
    (0x26a7, 0x26a7),
    (0x26b0, 0x26b1),
    (0x26c8, 0x26c8),
    (0x26cf, 0x26cf),
    (0x26d1, 0x26d1),
    (0x26d3, 0x26d3),
    (0x26e9, 0x26e9),
    (0x26f0, 0x26f1),
    (0x26f4, 0x26f4),
    (0x26f7, 0x26f9),
    (0x2702, 0x2702),
    (0x2708, 0x2709),
    (0x270c, 0x270d),
    (0x270f, 0x270f),
    (0x2712, 0x2712),
    (0x2714, 0x2714),
    (0x2716, 0x2716),
    (0x271d, 0x271d),
    (0x2721, 0x2721),
    (0x2733, 0x2734),
    (0x2744, 0x2744),
    (0x2747, 0x2747),
    (0x2763, 0x2764),
    (0x27a1, 0x27a1),
    (0x2934, 0x2935),
    (0x2b05, 0x2b07),
    (0x1f170, 0x1f171),
    (0x1f17e, 0x1f17f),
    (0x1f321, 0x1f321),
    (0x1f324, 0x1f32c),
    (0x1f336, 0x1f336),
    (0x1f37d, 0x1f37d),
    (0x1f396, 0x1f397),
    (0x1f399, 0x1f39b),
    (0x1f39e, 0x1f39f),
    (0x1f3cb, 0x1f3ce),
    (0x1f3d4, 0x1f3df),
    (0x1f3f3, 0x1f3f3),
    (0x1f3f5, 0x1f3f5),
    (0x1f3f7, 0x1f3f7),
    (0x1f43f, 0x1f43f),
    (0x1f441, 0x1f441),
    (0x1f4fd, 0x1f4fd),
    (0x1f549, 0x1f54a),
    (0x1f56f, 0x1f570),
    (0x1f573, 0x1f579),
    (0x1f587, 0x1f587),
    (0x1f58a, 0x1f58d),
    (0x1f590, 0x1f590),
    (0x1f5a5, 0x1f5a5),
    (0x1f5a8, 0x1f5a8),
    (0x1f5b1, 0x1f5b2),
    (0x1f5bc, 0x1f5bc),
    (0x1f5c2, 0x1f5c4),
    (0x1f5d1, 0x1f5d3),
    (0x1f5dc, 0x1f5de),
    (0x1f5e1, 0x1f5e1),
    (0x1f5e3, 0x1f5e3),
    (0x1f5e8, 0x1f5e8),
    (0x1f5ef, 0x1f5ef),
    (0x1f5f3, 0x1f5f3),
    (0x1f5fa, 0x1f5fa),
    (0x1f6cb, 0x1f6cb),
    (0x1f6cd, 0x1f6cf),
    (0x1f6e0, 0x1f6e5),
    (0x1f6e9, 0x1f6e9),
    (0x1f6f0, 0x1f6f0),
    (0x1f6f3, 0x1f6f3),
];

const ZERO_WIDTH_RANGES: &[(u32, u32)] = &[
    (0xad, 0xad),
    (0x300, 0x36f),
    (0x483, 0x489),
    (0x591, 0x5bd),
    (0x5bf, 0x5bf),
    (0x5c1, 0x5c2),
    (0x5c4, 0x5c5),
    (0x5c7, 0x5c7),
    (0x605, 0x605),
    (0x610, 0x61a),
    (0x61c, 0x61c),
    (0x64b, 0x65f),
    (0x670, 0x670),
    (0x6d6, 0x6dc),
    (0x6df, 0x6e4),
    (0x6e7, 0x6e8),
    (0x6ea, 0x6ed),
    (0x70f, 0x70f),
    (0x711, 0x711),
    (0x730, 0x74a),
    (0x7a6, 0x7b0),
    (0x7eb, 0x7f3),
    (0x7fd, 0x7fd),
    (0x816, 0x819),
    (0x81b, 0x823),
    (0x825, 0x827),
    (0x829, 0x82d),
    (0x859, 0x85b),
    (0x890, 0x891),
    (0x897, 0x89f),
    (0x8ca, 0x902),
    (0x93a, 0x93a),
    (0x93c, 0x93c),
    (0x941, 0x948),
    (0x94d, 0x94d),
    (0x951, 0x957),
    (0x962, 0x963),
    (0x981, 0x981),
    (0x9bc, 0x9bc),
    (0x9be, 0x9be),
    (0x9c1, 0x9c4),
    (0x9cd, 0x9cd),
    (0x9d7, 0x9d7),
    (0x9e2, 0x9e3),
    (0x9fe, 0x9fe),
    (0xa01, 0xa02),
    (0xa3c, 0xa3c),
    (0xa41, 0xa42),
    (0xa47, 0xa48),
    (0xa4b, 0xa4d),
    (0xa51, 0xa51),
    (0xa70, 0xa71),
    (0xa75, 0xa75),
    (0xa81, 0xa82),
    (0xabc, 0xabc),
    (0xac1, 0xac5),
    (0xac7, 0xac8),
    (0xacd, 0xacd),
    (0xae2, 0xae3),
    (0xafa, 0xaff),
    (0xb01, 0xb01),
    (0xb3c, 0xb3c),
    (0xb3e, 0xb3f),
    (0xb41, 0xb44),
    (0xb4d, 0xb4d),
    (0xb55, 0xb57),
    (0xb62, 0xb63),
    (0xb82, 0xb82),
    (0xbbe, 0xbbe),
    (0xbc0, 0xbc0),
    (0xbcd, 0xbcd),
    (0xbd7, 0xbd7),
    (0xc00, 0xc00),
    (0xc04, 0xc04),
    (0xc3c, 0xc3c),
    (0xc3e, 0xc40),
    (0xc46, 0xc48),
    (0xc4a, 0xc4d),
    (0xc55, 0xc56),
    (0xc62, 0xc63),
    (0xc81, 0xc81),
    (0xcbc, 0xcbc),
    (0xcbf, 0xcc0),
    (0xcc2, 0xcc2),
    (0xcc6, 0xcc8),
    (0xcca, 0xccd),
    (0xcd5, 0xcd6),
    (0xce2, 0xce3),
    (0xd00, 0xd01),
    (0xd3b, 0xd3c),
    (0xd3e, 0xd3e),
    (0xd41, 0xd44),
    (0xd4d, 0xd4e),
    (0xd57, 0xd57),
    (0xd62, 0xd63),
    (0xd81, 0xd81),
    (0xdca, 0xdca),
    (0xdcf, 0xdcf),
    (0xdd2, 0xdd4),
    (0xdd6, 0xdd6),
    (0xddf, 0xddf),
    (0xe31, 0xe31),
    (0xe34, 0xe3a),
    (0xe47, 0xe4e),
    (0xeb1, 0xeb1),
    (0xeb4, 0xebc),
    (0xec8, 0xece),
    (0xf18, 0xf19),
    (0xf35, 0xf35),
    (0xf37, 0xf37),
    (0xf39, 0xf39),
    (0xf71, 0xf7e),
    (0xf80, 0xf84),
    (0xf86, 0xf87),
    (0xf8d, 0xf97),
    (0xf99, 0xfbc),
    (0xfc6, 0xfc6),
    (0x102d, 0x1030),
    (0x1032, 0x1037),
    (0x1039, 0x103a),
    (0x103d, 0x103e),
    (0x1058, 0x1059),
    (0x105e, 0x1060),
    (0x1071, 0x1074),
    (0x1082, 0x1082),
    (0x1085, 0x1086),
    (0x108d, 0x108d),
    (0x109d, 0x109d),
    (0x1160, 0x11ff),
    (0x135d, 0x135f),
    (0x1712, 0x1715),
    (0x1732, 0x1734),
    (0x1752, 0x1753),
    (0x1772, 0x1773),
    (0x17b4, 0x17b5),
    (0x17b7, 0x17bd),
    (0x17c6, 0x17c6),
    (0x17c9, 0x17d3),
    (0x17dd, 0x17dd),
    (0x180b, 0x180f),
    (0x1885, 0x1886),
    (0x18a9, 0x18a9),
    (0x1920, 0x1922),
    (0x1927, 0x1928),
    (0x1932, 0x1932),
    (0x1939, 0x193b),
    (0x1a17, 0x1a18),
    (0x1a1b, 0x1a1b),
    (0x1a56, 0x1a56),
    (0x1a58, 0x1a5e),
    (0x1a60, 0x1a60),
    (0x1a62, 0x1a62),
    (0x1a65, 0x1a6c),
    (0x1a73, 0x1a7c),
    (0x1a7f, 0x1a7f),
    (0x1ab0, 0x1add),
    (0x1ae0, 0x1aeb),
    (0x1b00, 0x1b03),
    (0x1b34, 0x1b3d),
    (0x1b42, 0x1b44),
    (0x1b6b, 0x1b73),
    (0x1b80, 0x1b81),
    (0x1ba2, 0x1ba5),
    (0x1ba8, 0x1bad),
    (0x1be6, 0x1be6),
    (0x1be8, 0x1be9),
    (0x1bed, 0x1bed),
    (0x1bef, 0x1bf3),
    (0x1c2c, 0x1c33),
    (0x1c36, 0x1c37),
    (0x1cd0, 0x1cd2),
    (0x1cd4, 0x1ce0),
    (0x1ce2, 0x1ce8),
    (0x1ced, 0x1ced),
    (0x1cf4, 0x1cf4),
    (0x1cf8, 0x1cf9),
    (0x1dc0, 0x1dff),
    (0x200b, 0x200f),
    (0x202a, 0x202e),
    (0x2060, 0x206f),
    (0x20d0, 0x20f0),
    (0x2cef, 0x2cf1),
    (0x2de0, 0x2dff),
    (0x302a, 0x302f),
    (0x3099, 0x309a),
    (0x3164, 0x3164),
    (0xa66f, 0xa672),
    (0xa674, 0xa67d),
    (0xa69e, 0xa69f),
    (0xa6f0, 0xa6f1),
    (0xa802, 0xa802),
    (0xa806, 0xa806),
    (0xa80b, 0xa80b),
    (0xa825, 0xa826),
    (0xa82c, 0xa82c),
    (0xa8c4, 0xa8c5),
    (0xa8e0, 0xa8f1),
    (0xa8fa, 0xa8fa),
    (0xa8ff, 0xa8ff),
    (0xa926, 0xa92d),
    (0xa947, 0xa951),
    (0xa953, 0xa953),
    (0xa980, 0xa982),
    (0xa9b3, 0xa9b3),
    (0xa9b6, 0xa9b9),
    (0xa9bc, 0xa9bd),
    (0xa9c0, 0xa9c0),
    (0xa9e5, 0xa9e5),
    (0xaa29, 0xaa2e),
    (0xaa31, 0xaa32),
    (0xaa35, 0xaa36),
    (0xaa43, 0xaa43),
    (0xaa4c, 0xaa4c),
    (0xaa7c, 0xaa7c),
    (0xaab0, 0xaab0),
    (0xaab2, 0xaab4),
    (0xaab7, 0xaab8),
    (0xaabe, 0xaabf),
    (0xaac1, 0xaac1),
    (0xaaec, 0xaaed),
    (0xaaf6, 0xaaf6),
    (0xabe5, 0xabe5),
    (0xabe8, 0xabe8),
    (0xabed, 0xabed),
    (0xd7b0, 0xd7c6),
    (0xd7cb, 0xd7fb),
    (0xfb1e, 0xfb1e),
    (0xfe00, 0xfe0f),
    (0xfe20, 0xfe2f),
    (0xfeff, 0xfeff),
    (0xff9e, 0xffa0),
    (0xfff0, 0xfff8),
    (0x101fd, 0x101fd),
    (0x102e0, 0x102e0),
    (0x10376, 0x1037a),
    (0x10a01, 0x10a03),
    (0x10a05, 0x10a06),
    (0x10a0c, 0x10a0f),
    (0x10a38, 0x10a3a),
    (0x10a3f, 0x10a3f),
    (0x10ae5, 0x10ae6),
    (0x10d24, 0x10d27),
    (0x10d69, 0x10d6d),
    (0x10eab, 0x10eac),
    (0x10efa, 0x10eff),
    (0x10f46, 0x10f50),
    (0x10f82, 0x10f85),
    (0x11001, 0x11001),
    (0x11038, 0x11046),
    (0x11070, 0x11070),
    (0x11073, 0x11074),
    (0x1107f, 0x11081),
    (0x110b3, 0x110b6),
    (0x110b9, 0x110ba),
    (0x110c2, 0x110c2),
    (0x11100, 0x11102),
    (0x11127, 0x1112b),
    (0x1112d, 0x11134),
    (0x11173, 0x11173),
    (0x11180, 0x11181),
    (0x111b6, 0x111be),
    (0x111c0, 0x111c0),
    (0x111c2, 0x111c3),
    (0x111c9, 0x111cc),
    (0x111cf, 0x111cf),
    (0x1122f, 0x11231),
    (0x11234, 0x11237),
    (0x1123e, 0x1123e),
    (0x11241, 0x11241),
    (0x112df, 0x112df),
    (0x112e3, 0x112ea),
    (0x11300, 0x11301),
    (0x1133b, 0x1133c),
    (0x1133e, 0x1133e),
    (0x11340, 0x11340),
    (0x1134d, 0x1134d),
    (0x11357, 0x11357),
    (0x11366, 0x1136c),
    (0x11370, 0x11374),
    (0x113b8, 0x113b8),
    (0x113bb, 0x113c0),
    (0x113c2, 0x113c2),
    (0x113c5, 0x113c5),
    (0x113c7, 0x113c9),
    (0x113ce, 0x113d2),
    (0x113e1, 0x113e2),
    (0x11438, 0x1143f),
    (0x11442, 0x11444),
    (0x11446, 0x11446),
    (0x1145e, 0x1145e),
    (0x114b0, 0x114b0),
    (0x114b3, 0x114b8),
    (0x114ba, 0x114ba),
    (0x114bd, 0x114bd),
    (0x114bf, 0x114c0),
    (0x114c2, 0x114c3),
    (0x115af, 0x115af),
    (0x115b2, 0x115b5),
    (0x115bc, 0x115bd),
    (0x115bf, 0x115c0),
    (0x115dc, 0x115dd),
    (0x11633, 0x1163a),
    (0x1163d, 0x1163d),
    (0x1163f, 0x11640),
    (0x116ab, 0x116ab),
    (0x116ad, 0x116ad),
    (0x116b0, 0x116b7),
    (0x1171d, 0x1171d),
    (0x1171f, 0x1171f),
    (0x11722, 0x11725),
    (0x11727, 0x1172b),
    (0x1182f, 0x11837),
    (0x11839, 0x1183a),
    (0x11930, 0x11930),
    (0x1193b, 0x1193f),
    (0x11941, 0x11941),
    (0x11943, 0x11943),
    (0x119d4, 0x119d7),
    (0x119da, 0x119db),
    (0x119e0, 0x119e0),
    (0x11a01, 0x11a0a),
    (0x11a33, 0x11a38),
    (0x11a3b, 0x11a3e),
    (0x11a47, 0x11a47),
    (0x11a51, 0x11a56),
    (0x11a59, 0x11a5b),
    (0x11a84, 0x11a96),
    (0x11a98, 0x11a99),
    (0x11b60, 0x11b60),
    (0x11b62, 0x11b64),
    (0x11b66, 0x11b66),
    (0x11c30, 0x11c36),
    (0x11c38, 0x11c3d),
    (0x11c3f, 0x11c3f),
    (0x11c92, 0x11ca7),
    (0x11caa, 0x11cb0),
    (0x11cb2, 0x11cb3),
    (0x11cb5, 0x11cb6),
    (0x11d31, 0x11d36),
    (0x11d3a, 0x11d3a),
    (0x11d3c, 0x11d3d),
    (0x11d3f, 0x11d47),
    (0x11d90, 0x11d91),
    (0x11d95, 0x11d95),
    (0x11d97, 0x11d97),
    (0x11ef3, 0x11ef4),
    (0x11f00, 0x11f02),
    (0x11f36, 0x11f3a),
    (0x11f40, 0x11f42),
    (0x11f5a, 0x11f5a),
    (0x13440, 0x13440),
    (0x13447, 0x13455),
    (0x1611e, 0x16129),
    (0x1612d, 0x1612f),
    (0x16af0, 0x16af4),
    (0x16b30, 0x16b36),
    (0x16f4f, 0x16f4f),
    (0x16f8f, 0x16f92),
    (0x16fe4, 0x16fe4),
    (0x16ff0, 0x16ff1),
    (0x1bc9d, 0x1bc9e),
    (0x1bca0, 0x1bca3),
    (0x1cf00, 0x1cf2d),
    (0x1cf30, 0x1cf46),
    (0x1d165, 0x1d169),
    (0x1d16d, 0x1d182),
    (0x1d185, 0x1d18b),
    (0x1d1aa, 0x1d1ad),
    (0x1d242, 0x1d244),
    (0x1da00, 0x1da36),
    (0x1da3b, 0x1da6c),
    (0x1da75, 0x1da75),
    (0x1da84, 0x1da84),
    (0x1da9b, 0x1da9f),
    (0x1daa1, 0x1daaf),
    (0x1e000, 0x1e006),
    (0x1e008, 0x1e018),
    (0x1e01b, 0x1e021),
    (0x1e023, 0x1e024),
    (0x1e026, 0x1e02a),
    (0x1e08f, 0x1e08f),
    (0x1e130, 0x1e136),
    (0x1e2ae, 0x1e2ae),
    (0x1e2ec, 0x1e2ef),
    (0x1e4ec, 0x1e4ef),
    (0x1e5ee, 0x1e5ef),
    (0x1e6e3, 0x1e6e3),
    (0x1e6e6, 0x1e6e6),
    (0x1e6ee, 0x1e6ef),
    (0x1e6f5, 0x1e6f5),
    (0x1e8d0, 0x1e8d6),
    (0x1e944, 0x1e94a),
    (0xe0000, 0xe0fff),
];
const WIDE_RANGES: &[(u32, u32)] = &[
    (0x1100, 0x115f),
    (0x17a4, 0x17a4),
    (0x231a, 0x231b),
    (0x2329, 0x232a),
    (0x23e9, 0x23ec),
    (0x23f0, 0x23f0),
    (0x23f3, 0x23f3),
    (0x25fd, 0x25fe),
    (0x2614, 0x2615),
    (0x2630, 0x2637),
    (0x2648, 0x2653),
    (0x267f, 0x267f),
    (0x268a, 0x268f),
    (0x2693, 0x2693),
    (0x26a1, 0x26a1),
    (0x26aa, 0x26ab),
    (0x26bd, 0x26be),
    (0x26c4, 0x26c5),
    (0x26ce, 0x26ce),
    (0x26d4, 0x26d4),
    (0x26ea, 0x26ea),
    (0x26f2, 0x26f3),
    (0x26f5, 0x26f5),
    (0x26fa, 0x26fa),
    (0x26fd, 0x26fd),
    (0x2705, 0x2705),
    (0x270a, 0x270b),
    (0x2728, 0x2728),
    (0x274c, 0x274c),
    (0x274e, 0x274e),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27b0, 0x27b0),
    (0x27bf, 0x27bf),
    (0x2b1b, 0x2b1c),
    (0x2b50, 0x2b50),
    (0x2b55, 0x2b55),
    (0x2e80, 0x2e99),
    (0x2e9b, 0x2ef3),
    (0x2f00, 0x2fd5),
    (0x2ff0, 0x3029),
    (0x3030, 0x303e),
    (0x3041, 0x3096),
    (0x309b, 0x30ff),
    (0x3105, 0x312f),
    (0x3131, 0x3163),
    (0x3165, 0x318e),
    (0x3190, 0x31e5),
    (0x31ef, 0x321e),
    (0x3220, 0x3247),
    (0x3250, 0xa48c),
    (0xa490, 0xa4c6),
    (0xa960, 0xa97c),
    (0xac00, 0xd7a3),
    (0xf900, 0xfaff),
    (0xfe10, 0xfe19),
    (0xfe30, 0xfe52),
    (0xfe54, 0xfe66),
    (0xfe68, 0xfe6b),
    (0xff01, 0xff60),
    (0xffe0, 0xffe6),
    (0x16fe0, 0x16fe3),
    (0x16ff2, 0x16ff6),
    (0x17000, 0x18cd5),
    (0x18cff, 0x18d1e),
    (0x18d80, 0x18df2),
    (0x1aff0, 0x1aff3),
    (0x1aff5, 0x1affb),
    (0x1affd, 0x1affe),
    (0x1b000, 0x1b122),
    (0x1b132, 0x1b132),
    (0x1b150, 0x1b152),
    (0x1b155, 0x1b155),
    (0x1b164, 0x1b167),
    (0x1b170, 0x1b2fb),
    (0x1d300, 0x1d356),
    (0x1d360, 0x1d376),
    (0x1f004, 0x1f004),
    (0x1f0cf, 0x1f0cf),
    (0x1f18e, 0x1f18e),
    (0x1f191, 0x1f19a),
    (0x1f200, 0x1f202),
    (0x1f210, 0x1f23b),
    (0x1f240, 0x1f248),
    (0x1f250, 0x1f251),
    (0x1f260, 0x1f265),
    (0x1f300, 0x1f320),
    (0x1f32d, 0x1f335),
    (0x1f337, 0x1f37c),
    (0x1f37e, 0x1f393),
    (0x1f3a0, 0x1f3ca),
    (0x1f3cf, 0x1f3d3),
    (0x1f3e0, 0x1f3f0),
    (0x1f3f4, 0x1f3f4),
    (0x1f3f8, 0x1f43e),
    (0x1f440, 0x1f440),
    (0x1f442, 0x1f4fc),
    (0x1f4ff, 0x1f53d),
    (0x1f54b, 0x1f54e),
    (0x1f550, 0x1f567),
    (0x1f57a, 0x1f57a),
    (0x1f595, 0x1f596),
    (0x1f5a4, 0x1f5a4),
    (0x1f5fb, 0x1f64f),
    (0x1f680, 0x1f6c5),
    (0x1f6cc, 0x1f6cc),
    (0x1f6d0, 0x1f6d2),
    (0x1f6d5, 0x1f6d8),
    (0x1f6dc, 0x1f6df),
    (0x1f6eb, 0x1f6ec),
    (0x1f6f4, 0x1f6fc),
    (0x1f7e0, 0x1f7eb),
    (0x1f7f0, 0x1f7f0),
    (0x1f90c, 0x1f93a),
    (0x1f93c, 0x1f945),
    (0x1f947, 0x1f9ff),
    (0x1fa70, 0x1fa7c),
    (0x1fa80, 0x1fa8a),
    (0x1fa8e, 0x1fac6),
    (0x1fac8, 0x1fac8),
    (0x1facd, 0x1fadc),
    (0x1fadf, 0x1faea),
    (0x1faef, 0x1faf8),
    (0x20000, 0x2fffd),
    (0x30000, 0x3fffd),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_ascii_wide_emoji_and_combining_text() {
        assert_eq!(str_width("plain"), 5);
        assert_eq!(str_width("日本語"), 6);
        assert_eq!(str_width("🙂"), 2);
        assert_eq!(str_width("e\u{301}"), 1);
        assert_eq!(char_width('\n'), None);
    }

    #[test]
    fn measures_sequence_widths_instead_of_summing_scalars() {
        assert_eq!(str_width("👩‍💻"), 2);
        assert_eq!(str_width("👨‍👩‍👧‍👦"), 2);
        assert_eq!(str_width("👩🏽"), 2);
        assert_eq!(str_width("#️⃣"), 2);
        assert_eq!(str_width("א\u{200d}ל"), 1);
        assert_eq!(str_width("لا"), 1);
        assert_eq!(str_width("ល្ង"), 1);
        assert_eq!(str_width("🇵🇸\u{200d}🕊️\u{200d}🇮🇱"), 2);
        assert_eq!(str_width("🇦🇦\u{200d}🇦🇦"), 2);
        assert_eq!(str_width("♈\u{fe0e}"), 1);
        assert_eq!(str_width("👩\u{fe0e}"), 2);
        assert_eq!(str_width("💻🏿"), 4);
        assert_eq!(str_width("🏿🏿"), 4);
        assert_eq!(str_width("🇦🏿"), 3);
        assert_eq!(str_width("🇦\u{200d}👩"), 3);
        assert_eq!(str_width("ⵏ⵿ⴾ"), 1);
        assert_eq!(str_width("‘\u{fe01}"), 2);
        assert_eq!(str_width("𖵩"), 1);
        assert_eq!(str_width("𖵪"), 1);
        assert_eq!(str_width("𖵨"), 1);
        assert_eq!(width_chars("👩‍💻").map(|(_, width)| width).sum::<usize>(), 2);
    }

    #[test]
    fn covers_hangul_jamo_and_unicode_zero_width_classes() {
        assert_eq!(str_width("한"), 2);
        assert_eq!(char_width('\u{1161}'), Some(0));
        assert_eq!(char_width('\u{11ab}'), Some(0));
        assert_eq!(char_width('\u{d7c6}'), Some(0));
        assert_eq!(char_width('\u{d7fb}'), Some(0));
        assert_eq!(char_width('\u{09be}'), Some(0));
        assert_eq!(char_width('\u{00ad}'), Some(0));
        assert_eq!(char_width('\u{e0000}'), Some(0));
    }
}
