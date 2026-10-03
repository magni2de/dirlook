const WIDE_RANGES: [(u32, u32); 11] = [
    (0x1100, 0x115F),   // Hangul Jamo
    (0x2E80, 0x303E),   // CJK radicals + CJK punctuation
    (0x3041, 0x33FF),   // Hiragana, Katakana, CJK symbols
    (0x3400, 0x4DBF),   // CJK Extension A
    (0x4E00, 0x9FFF),   // CJK Unified Ideographs
    (0xAC00, 0xD7A3),   // Hangul Syllables
    (0xF900, 0xFAFF),   // CJK Compatibility Ideographs
    (0xFF00, 0xFF60),   // Fullwidth ASCII
    (0xFFE0, 0xFFE6),   // Fullwidth Symbols
    (0x1F300, 0x1FBFF), // Emoji
    (0x20000, 0x2FFFD), // CJK Extension B
];

pub fn char_width(c: char) -> usize {
    let cp = c as u32;
    if WIDE_RANGES.iter().any(|&(lo, hi)| cp >= lo && cp <= hi) {
        2
    } else {
        1
    }
}

pub fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Truncate `s` so its display width is at most `max_width` cells.
pub fn truncate(s: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw > max_width {
            break;
        }
        result.push(c);
        w += cw;
    }
    result
}

/// Pad `s` to exactly `width` display cells.
/// `right_align=false` → prepend nothing, append spaces (left-aligned).
/// `right_align=true`  → prepend spaces, append nothing (right-aligned).
pub fn pad(s: &str, width: usize, right_align: bool) -> String {
    let cur = str_width(s);
    if cur >= width {
        return truncate(s, width);
    }
    let fill = width - cur;
    if right_align {
        format!("{}{}", " ".repeat(fill), s)
    } else {
        format!("{}{}", s, " ".repeat(fill))
    }
}
