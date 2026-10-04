use crate::color;
use crate::screen::{Screen, DEFAULT_BG};
use crate::width;

pub const OTHER_COLOR: (u8, u8, u8) = (110, 110, 120);
/// Colour for directories whose size is not known yet.
pub const UNKNOWN_COLOR: (u8, u8, u8) = (58, 58, 66);

const MIN_SHARE_FRAC: f64 = 0.01;

#[derive(Clone)]
pub struct MapEntry {
    pub label: String,
    pub size: u64,
    pub known: bool,
    pub is_dir: bool,
    pub color: (u8, u8, u8),
    pub highlight: bool,
}

#[derive(Clone)]
pub struct Block {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub color: (u8, u8, u8),
    pub label: String,
    pub size: u64,
    pub known: bool,
    pub highlight: bool,
}

#[derive(Clone)]
pub struct Others {
    pub count: usize,
    pub size: u64,
}

#[derive(Default, Clone)]
pub struct Layout {
    pub blocks: Vec<Block>,
    pub others: Option<Others>,
}

/// Compute a one-level treemap layout for `entries` in a `w`×`h` cell region.
pub fn compute(w: usize, h: usize, entries: &[MapEntry]) -> Layout {
    if w == 0 || h == 0 || entries.is_empty() {
        return Layout::default();
    }

    let mut sorted: Vec<&MapEntry> = entries.iter().collect();
    let is_unknown = |e: &MapEntry| e.is_dir && !e.known;
    // Unknown directories first (they must always be shown), then by size desc.
    sorted.sort_by(|a, b| {
        is_unknown(b)
            .cmp(&is_unknown(a))
            .then_with(|| b.size.cmp(&a.size))
    });

    let total: u64 = sorted.iter().map(|e| e.size).sum();
    let cells = w * h;
    let max_known = sorted
        .iter()
        .filter(|e| !is_unknown(e))
        .map(|e| e.size)
        .max()
        .unwrap_or(0);
    let has_big = total > 0 && max_known as f64 / total as f64 >= MIN_SHARE_FRAC;

    let mut keep = 0usize;
    for e in &sorted {
        let frac = if total > 0 {
            e.size as f64 / total as f64
        } else {
            0.0
        };
        let keepit = is_unknown(e)
            || if has_big {
                frac >= MIN_SHARE_FRAC
            } else {
                frac * cells as f64 >= 1.0
            };
        if keepit {
            keep += 1;
        } else {
            break;
        }
    }
    if keep == 0 {
        keep = 1;
    }

    let sizes: Vec<f64> = sorted
        .iter()
        .take(keep)
        .map(|e| e.size.max(1) as f64)
        .collect();
    let rects = layout(&sizes, 0.0, 0.0, w as f64, h as f64);

    let mut blocks = Vec::with_capacity(rects.len());
    for (idx, r) in rects.iter().enumerate() {
        let (x, y, bw, bh) = to_cells(*r, w, h);
        if bw == 0 || bh == 0 {
            continue;
        }
        let e = sorted[idx];
        let color = if e.is_dir && !e.known {
            UNKNOWN_COLOR
        } else {
            e.color
        };
        blocks.push(Block {
            x,
            y,
            w: bw,
            h: bh,
            color,
            label: e.label.clone(),
            size: e.size,
            known: e.known,
            highlight: e.highlight,
        });
    }

    let others = if keep < sorted.len() {
        Some(Others {
            count: sorted.len() - keep,
            size: sorted[keep..].iter().map(|e| e.size).sum(),
        })
    } else {
        None
    };

    Layout { blocks, others }
}

pub fn draw(screen: &mut Screen, ox: usize, oy: usize, w: usize, h: usize, layout: &Layout) {
    if w == 0 || h == 0 {
        return;
    }
    screen.fill_bg(ox, oy, w, h, DEFAULT_BG);

    if layout.blocks.is_empty() && layout.others.is_none() {
        let msg = width::truncate("(empty)", w);
        let cx = ox + w.saturating_sub(width::str_width(&msg)) / 2;
        let cy = oy + h / 2;
        screen.put_str(cx, cy, &msg, (160, 160, 160), DEFAULT_BG);
        return;
    }

    for b in &layout.blocks {
        let sx = ox + b.x;
        let sy = oy + b.y;
        let xr = (sx + b.w).min(ox + w);
        let yr = (sy + b.h).min(oy + h);
        screen.fill_bg(sx, sy, b.w, b.h, b.color);

        if !b.known {
            // Unknown: fill with question marks to signal "not scanned yet".
            let fg = (96, 96, 106);
            for yy in sy..yr {
                for xx in sx..xr {
                    screen.put(xx, yy, '?', fg, UNKNOWN_COLOR);
                }
            }
        } else {
            let frame = (b.w >= 4 && b.h >= 3) || (b.highlight && b.w >= 3 && b.h >= 3);
            if frame {
                let (br, bg2, bb) = if b.highlight {
                    (255, 255, 255)
                } else {
                    (24, 24, 28)
                };
                for xx in sx..xr {
                    screen.put(xx, sy, '─', (br, bg2, bb), b.color);
                    screen.put(xx, sy + b.h - 1, '─', (br, bg2, bb), b.color);
                }
                for yy in sy..yr {
                    screen.put(sx, yy, '│', (br, bg2, bb), b.color);
                    screen.put(sx + b.w - 1, yy, '│', (br, bg2, bb), b.color);
                }
                screen.put(sx, sy, '┌', (br, bg2, bb), b.color);
                screen.put(sx + b.w - 1, sy, '┐', (br, bg2, bb), b.color);
                screen.put(sx, sy + b.h - 1, '└', (br, bg2, bb), b.color);
                screen.put(sx + b.w - 1, sy + b.h - 1, '┘', (br, bg2, bb), b.color);
            }

            if b.w >= 4 && b.h >= 1 {
                let (tx, ty, tw, th) = if frame {
                    (sx + 1, sy + 1, b.w.saturating_sub(2), b.h.saturating_sub(2))
                } else {
                    (sx, sy, b.w, b.h)
                };
                if tw >= 3 && th >= 1 {
                    let dark = luminance(b.color.0, b.color.1, b.color.2) < 128;
                    let fg_name = if dark { (245, 245, 245) } else { (20, 20, 20) };
                    let fg_size = color::text_shade(b.color);
                    let name = width::truncate(&b.label, tw);
                    screen.put_str(tx, ty, &name, fg_name, b.color);
                    if th >= 2 {
                        let sz = color::human_size(b.size);
                        screen.put_str(tx, ty + 1, &sz, fg_size, b.color);
                    }
                }
            }
        }
    }

    if let Some(others) = &layout.others {
        let label = format!(
            "⋯ others ({} · {})",
            others.count,
            color::human_size(others.size)
        );
        draw_chip(screen, ox, oy, w, h, &label);
    }
}

fn draw_chip(screen: &mut Screen, ox: usize, oy: usize, w: usize, h: usize, label: &str) {
    let text = width::truncate(label, w.saturating_sub(4));
    let text_w = width::str_width(&text);
    let Some((x, y, cw, ch)) = chip_rect(w, h, text_w) else {
        return;
    };
    let sx = ox + x;
    let sy = oy + y;
    screen.fill_bg(sx, sy, cw, ch, OTHER_COLOR);
    if text_w >= 1 {
        let fg = (235, 235, 235);
        screen.put_str(sx + 1, sy, &text, fg, OTHER_COLOR);
    }
}

fn to_cells(r: (f64, f64, f64, f64), bw: usize, bh: usize) -> (usize, usize, usize, usize) {
    if bw == 0 || bh == 0 {
        return (0, 0, 0, 0);
    }
    let mut x = r.0.round().max(0.0) as usize;
    let mut y = r.1.round().max(0.0) as usize;
    let x1 = (r.0 + r.2).round().max(0.0) as usize;
    let y1 = (r.1 + r.3).round().max(0.0) as usize;
    if x >= bw {
        x = bw - 1;
    }
    if y >= bh {
        y = bh - 1;
    }
    let mut wq = x1.saturating_sub(x);
    let mut hq = y1.saturating_sub(y);
    if wq == 0 {
        wq = 1;
    }
    if hq == 0 {
        hq = 1;
    }
    if x + wq > bw {
        wq = bw - x;
    }
    if y + hq > bh {
        hq = bh - y;
    }
    (x, y, wq, hq)
}

fn chip_rect(bw: usize, bh: usize, text_w: usize) -> Option<(usize, usize, usize, usize)> {
    if bw < 5 || bh < 2 {
        return None;
    }
    let cw = (text_w + 2).min(bw - 2).max(1);
    let x = bw - cw - 1;
    let y = bh - 2;
    Some((x, y, cw, 1))
}

fn luminance(r: u8, g: u8, b: u8) -> u32 {
    (2126 * r as u32 + 7152 * g as u32 + 722 * b as u32) / 10000
}

fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    if sum <= 0.0 || side <= 0.0 {
        return f64::INFINITY;
    }
    let rmax = row.iter().cloned().fold(0.0_f64, f64::max);
    let rmin = row.iter().cloned().fold(f64::INFINITY, f64::min);
    f64::max(
        (side * side * rmax) / (sum * sum),
        (sum * sum) / (side * side * rmin),
    )
}

fn layout(sizes: &[f64], x: f64, y: f64, w: f64, h: f64) -> Vec<(f64, f64, f64, f64)> {
    let mut rects = Vec::with_capacity(sizes.len());
    let total: f64 = sizes.iter().sum();
    if total <= 0.0 || w <= 0.0 || h <= 0.0 {
        return rects;
    }
    let scale = (w * h) / total;
    let areas: Vec<f64> = sizes.iter().map(|s| s * scale).collect();

    let mut cx = x;
    let mut cy = y;
    let mut cw = w;
    let mut ch = h;
    let mut i = 0usize;

    while i < areas.len() {
        let short = cw.min(ch);
        if short <= 0.0 {
            break;
        }
        let mut row: Vec<f64> = Vec::new();
        let mut row_sum = 0.0;
        while i < areas.len() {
            let c = areas[i];
            if row.is_empty() {
                row.push(c);
                row_sum += c;
                i += 1;
            } else {
                let cur = worst(&row, short);
                let mut probe = row.clone();
                probe.push(c);
                if worst(&probe, short) <= cur {
                    row.push(c);
                    row_sum += c;
                    i += 1;
                } else {
                    break;
                }
            }
        }

        if cw >= ch {
            let col_w = row_sum / ch;
            let mut oy = cy;
            for &a in &row {
                let rh = a / col_w;
                rects.push((cx, oy, col_w, rh));
                oy += rh;
            }
            cx += col_w;
            cw -= col_w;
        } else {
            let row_h = row_sum / cw;
            let mut ox = cx;
            for &a in &row {
                let rw = a / row_h;
                rects.push((ox, cy, rw, row_h));
                ox += rw;
            }
            cy += row_h;
            ch -= row_h;
        }
    }

    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(label: &str, size: u64, known: bool, is_dir: bool) -> MapEntry {
        MapEntry {
            label: label.to_string(),
            size,
            known,
            is_dir,
            color: (10, 20, 30),
            highlight: false,
        }
    }

    #[test]
    fn layout_fills_bounds() {
        let sizes = vec![10.0, 6.0, 4.0, 3.0, 2.0, 1.0];
        let (bw, bh) = (60.0, 20.0);
        let rects = layout(&sizes, 0.0, 0.0, bw, bh);
        assert_eq!(rects.len(), sizes.len());
        for (x, y, w, h) in &rects {
            assert!(*x >= -0.001 && *y >= -0.001);
            assert!(*x + *w <= bw + 0.001);
            assert!(*y + *h <= bh + 0.001);
            assert!(*w > 0.0 && *h > 0.0);
        }
    }

    #[test]
    fn unknown_dirs_are_kept() {
        let entries = vec![e("big", 50_000_000, true, false), e("scanning", 0, false, true)];
        let l = compute(60, 20, &entries);
        assert!(l.blocks.iter().any(|b| !b.known));
    }

    #[test]
    fn others_aggregate_tail() {
        let entries = vec![
            e("big", 50_000_000, true, false),
            e("big2", 47_000_000, true, false),
            e("a", 1, true, false),
            e("b", 2, true, false),
        ];
        let l = compute(60, 20, &entries);
        assert!(l.others.is_some());
    }
}
