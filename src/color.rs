pub const DIR_COLOR: (u8, u8, u8) = (94, 114, 176);

pub struct Category {
    pub name: &'static str,
    pub color: (u8, u8, u8),
    pub exts: &'static [&'static str],
}

pub const CATEGORIES: &[Category] = &[
    Category {
        name: "images",
        color: (76, 175, 80),
        exts: &[
            "jpg", "jpeg", "png", "gif", "bmp", "svg", "webp", "tiff", "tif", "heic", "ico",
        ],
    },
    Category {
        name: "video",
        color: (156, 39, 176),
        exts: &[
            "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg",
        ],
    },
    Category {
        name: "audio",
        color: (255, 152, 0),
        exts: &["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma", "opus"],
    },
    Category {
        name: "archives",
        color: (211, 47, 47),
        exts: &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "tgz", "iso"],
    },
    Category {
        name: "documents",
        color: (33, 150, 243),
        exts: &[
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "rtf", "odt", "csv",
            "log",
        ],
    },
    Category {
        name: "code",
        color: (0, 188, 212),
        exts: &[
            "rs", "js", "ts", "jsx", "tsx", "py", "c", "cpp", "cc", "h", "hpp", "go", "java",
            "kt", "rb", "php", "html", "css", "json", "toml", "yaml", "yml", "sh", "bash", "zsh",
            "xml",
        ],
    },
    Category {
        name: "binaries",
        color: (120, 130, 140),
        exts: &["exe", "dll", "so", "dylib", "bin", "o", "a", "lib", "obj"],
    },
];

pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = l - c / 2.0;
    let to = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r1), to(g1), to(b1))
}

/// WinDirStat-style palette: directories a fixed color, files by extension.
pub fn ext_color(name: &str, is_dir: bool) -> (u8, u8, u8) {
    if is_dir {
        return DIR_COLOR;
    }
    let lower = name.to_lowercase();
    let ext = lower
        .rsplit('.')
        .next()
        .filter(|e| *e != lower)
        .unwrap_or("");
    for cat in CATEGORIES {
        if cat.exts.contains(&ext) {
            return cat.color;
        }
    }
    let key = if ext.is_empty() { lower.as_str() } else { ext };
    let mut h: u32 = 5381;
    for b in key.bytes() {
        h = h.wrapping_mul(33) ^ b as u32;
    }
    hsl_to_rgb((h % 360) as f64, 0.5, 0.55)
}

/// Tinted, contrasting text color derived from a block color: darken light
/// blocks (multiply), lighten dark blocks (tint toward white), keeping the hue.
pub fn text_shade(rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let (r, g, b) = rgb;
    let lum = (2126 * r as u32 + 7152 * g as u32 + 722 * b as u32) / 10000;
    if lum >= 128 {
        (
            (r as f64 * 0.45) as u8,
            (g as f64 * 0.45) as u8,
            (b as f64 * 0.45) as u8,
        )
    } else {
        let mix = |v: u8| (v as f64 + (255.0 - v as f64) * 0.55) as u8;
        (mix(r), mix(g), mix(b))
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{} B", bytes);
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", value, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_shade_contrasts() {
        let (r, g, b) = (200, 220, 210);
        let (sr, sg, sb) = text_shade((r, g, b));
        assert!(sr < r && sg < g && sb < b, "light block must darken");

        let (r, g, b) = (80, 40, 120);
        let (sr, sg, sb) = text_shade((r, g, b));
        assert!(sr > r && sg > g && sb > b, "dark block must lighten");
    }

    #[test]
    fn categories_cover_common_exts() {
        assert_eq!(ext_color("photo.JPG", false), CATEGORIES[0].color);
        assert_eq!(ext_color("movie.mkv", false), CATEGORIES[1].color);
        assert_eq!(ext_color("main.rs", false), CATEGORIES[5].color);
        assert_eq!(ext_color("anything", true), DIR_COLOR);
    }
}
