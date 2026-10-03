use crate::color;
use crate::screen::{Screen, DEFAULT_BG};
use crate::width;

pub const OTHER_COLOR: (u8, u8, u8) = (110, 110, 120);

/// An item gets its own block only if it is at least this fraction of the total.
/// Smaller items — files *and* tiny directories alike — are folded into the
/// `⋯ others` chip. If nothing reaches the fraction (a folder of many equally
/// small items), we fall back to fitting every item that can hold one cell, so
/// uniform folders still render proportionally.
const MIN_SHARE_FRAC: f64 = 0.01;

pub struct Item {
    pub label: String,
    pub size: u64,
    pub color: (u8, u8, u8),
    pub highlight: bool,
}

pub struct Others {
    pub count: usize,
    pub size: u64,
}

#[derive(Default)]
pub struct Data {
    pub items: Vec<Item>,
    pub others: Option<Others>,
}

/// Split children into the items that get their own block and the aggregated
/// remainder. `entries` must be sorted by size descending; `highlight` indexes
/// `entries`.
pub fn build_data(
    cells: usize,
    entries: &[(String, u64, (u8, u8, u8))],
    highlight: Option<usize>,
) -> Data {
    if entries.is_empty() {
        return Data::default();
    }
    let total: u64 = entries.iter().map(|e| e.1).sum();

    let has_big = total > 0 && entries[0].1 as f64 / total as f64 >= MIN_SHARE_FRAC;

    let mut keep = 0usize;
    if total > 0 && cells > 0 {
        for e in entries.iter() {
            let frac = e.1 as f64 / total as f64;
            let keepit = if has_big {
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
    }
    if keep == 0 {
        keep = 1;
    }

    let items: Vec<Item> = entries
        .iter()
        .take(keep)
        .enumerate()
        .map(|(i, e)| Item {
            label: e.0.clone(),
            size: e.1,
            color: e.2,
            highlight: highlight == Some(i),
        })
        .collect();

    let others = if keep < entries.len() {
        Some(Others {
            count: entries.len() - keep,
            size: entries[keep..].iter().map(|e| e.1).sum(),
        })
    } else {
        None
    };

    Data { items, others }
}

/// Draw the proportional map at `(ox, oy)` (screen cells) and the "others" chip.
pub fn draw(screen: &mut Screen, ox: usize, oy: usize, bw: usize, bh: usize, data: &Data) {
    if bw == 0 || bh == 0 {
        return;
    }
    if data.items.is_empty() && data.others.is_none() {
        let msg = width::truncate("(empty)", bw);
        let cx = ox + bw.saturating_sub(width::str_width(&msg)) / 2;
        let cy = oy + bh / 2;
        screen.put_str(cx, cy, &msg, (160, 160, 160), DEFAULT_BG);
        return;
    }

    screen.fill_bg(ox, oy, bw, bh, DEFAULT_BG);

    if !data.items.is_empty() {
        let sizes: Vec<f64> = data
            .items
            .iter()
            .map(|it| it.size.max(1) as f64)
            .collect();
        let rects = layout(&sizes, 0.0, 0.0, bw as f64, bh as f64);

        for (idx, r) in rects.iter().enumerate() {
            let (x, y, rw, rh) = to_cells(*r, bw, bh);
            if rw == 0 || rh == 0 {
                continue;
            }
            let (cr, cg, cb) = data.items[idx].color;
            let hl = data.items[idx].highlight;
            let sx = ox + x;
            let sy = oy + y;
            let xr = (sx + rw).min(ox + bw);
            let yr = (sy + rh).min(oy + bh);

            screen.fill_bg(sx, sy, rw, rh, (cr, cg, cb));

            let frame = (rw >= 4 && rh >= 3) || (hl && rw >= 3 && rh >= 3);
            if frame {
                let (br, bg2, bb) = if hl { (255, 255, 255) } else { (24, 24, 28) };
                for xx in sx..xr {
                    screen.put(xx, sy, '─', (br, bg2, bb), (cr, cg, cb));
                    screen.put(xx, sy + rh - 1, '─', (br, bg2, bb), (cr, cg, cb));
                }
                for yy in sy..yr {
                    screen.put(sx, yy, '│', (br, bg2, bb), (cr, cg, cb));
                    screen.put(sx + rw - 1, yy, '│', (br, bg2, bb), (cr, cg, cb));
                }
                screen.put(sx, sy, '┌', (br, bg2, bb), (cr, cg, cb));
                screen.put(sx + rw - 1, sy, '┐', (br, bg2, bb), (cr, cg, cb));
                screen.put(sx, sy + rh - 1, '└', (br, bg2, bb), (cr, cg, cb));
                screen.put(sx + rw - 1, sy + rh - 1, '┘', (br, bg2, bb), (cr, cg, cb));
            }

            if rw >= 4 && rh >= 1 {
                let (tx, ty, tw, th) = if frame {
                    (
                        sx + 1,
                        sy + 1,
                        rw.saturating_sub(2),
                        rh.saturating_sub(2),
                    )
                } else {
                    (sx, sy, rw, rh)
                };
                if tw >= 3 && th >= 1 {
                    let dark = luminance(cr, cg, cb) < 128;
                    let fg_name = if dark { (245, 245, 245) } else { (20, 20, 20) };
                    let fg_size = color::text_shade((cr, cg, cb));
                    let name = width::truncate(&data.items[idx].label, tw);
                    screen.put_str(tx, ty, &name, fg_name, (cr, cg, cb));
                    if th >= 2 {
                        let sz = color::human_size(data.items[idx].size);
                        screen.put_str(tx, ty + 1, &sz, fg_size, (cr, cg, cb));
                    }
                }
            }
        }
    }

    if let Some(others) = &data.others {
        let label = format!(
            "⋯ others ({} · {})",
            others.count,
            color::human_size(others.size)
        );
        draw_chip(screen, ox, oy, bw, bh, &label);
    }
}

fn draw_chip(screen: &mut Screen, ox: usize, oy: usize, bw: usize, bh: usize, label: &str) {
    let text = width::truncate(label, bw.saturating_sub(4));
    let text_w = width::str_width(&text);
    let Some((x, y, cw, ch)) = chip_rect(bw, bh, text_w) else {
        return;
    };
    let sx = ox + x;
    let sy = oy + y;
    screen.fill_bg(sx, sy, cw, ch, OTHER_COLOR);
    if text_w >= 1 {
        let fg = if luminance(OTHER_COLOR.0, OTHER_COLOR.1, OTHER_COLOR.2) < 128 {
            (235, 235, 235)
        } else {
            (20, 20, 20)
        };
        screen.put_str(sx + 1, sy, &text, fg, OTHER_COLOR);
    }
}

/// Convert a float rectangle to integer cells, guaranteeing at least 1×1 so a
/// block never disappears to rounding, and clamping it inside the region.
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
    let mut w = x1.saturating_sub(x);
    let mut h = y1.saturating_sub(y);
    if w == 0 {
        w = 1;
    }
    if h == 0 {
        h = 1;
    }
    if x + w > bw {
        w = bw - x;
    }
    if y + h > bh {
        h = bh - y;
    }
    (x, y, w, h)
}

/// Rectangle for the "others" chip: bottom-right corner, inset one cell from
/// the edges so it never lands in the terminal's last column/row.
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

/// Squarified treemap layout. `sizes` must be sorted descending.
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

    fn e(name: &str, size: u64) -> (String, u64, (u8, u8, u8)) {
        (name.to_string(), size, (10, 20, 30))
    }

    #[test]
    fn layout_fills_bounds() {
        let sizes = vec![10.0, 6.0, 4.0, 3.0, 2.0, 1.0];
        let (bw, bh) = (60.0, 20.0);
        let rects = layout(&sizes, 0.0, 0.0, bw, bh);
        assert_eq!(rects.len(), sizes.len());
        for (x, y, w, h) in &rects {
            assert!(*x >= -0.001 && *y >= -0.001);
            assert!(*x + *w <= bw + 0.001, "overflow x: {} + {}", x, w);
            assert!(*y + *h <= bh + 0.001, "overflow y: {} + {}", y, h);
            assert!(*w > 0.0 && *h > 0.0);
        }
        let area: f64 = rects.iter().map(|r| r.2 * r.3).sum();
        assert!((area - bw * bh).abs() < 1.0, "area {area}");
    }

    #[test]
    fn layout_handles_single_and_empty() {
        assert!(layout(&[], 0.0, 0.0, 10.0, 10.0).is_empty());
        let one = layout(&[5.0], 0.0, 0.0, 10.0, 10.0);
        assert_eq!(one.len(), 1);
        assert!((one[0].2 - 10.0).abs() < 0.001);
        assert!((one[0].3 - 10.0).abs() < 0.001);
    }

    #[test]
    fn aggregates_tiny_items_into_others() {
        let entries = vec![
            e("big", 50_000_000),
            e("big2", 47_000_000),
            e("a", 1),
            e("b", 2),
            e("c", 3),
        ];
        let data = build_data(60 * 20, &entries, None);
        assert_eq!(data.items.len(), 2, "two big blocks only");
        let others = data.others.expect("aggregate");
        assert_eq!(others.count, 3);
        assert_eq!(others.size, 6);
    }

    #[test]
    fn keeps_at_least_one_when_all_tiny() {
        let entries: Vec<(String, u64, (u8, u8, u8))> =
            (0..200).map(|i| e(&format!("f{i}"), 1)).collect();
        let data = build_data(10 * 5, &entries, None);
        assert!(!data.items.is_empty());
        assert!(!data.items[0].label.contains("others"));
        assert!(data.others.is_some());
    }

    #[test]
    fn small_dirs_fold_into_others() {
        let entries = vec![e("big", 1000), e("dir_a", 5), e("dir_b", 3), e("file_c", 1)];
        let data = build_data(1000, &entries, None);
        assert_eq!(data.items.len(), 1, "only the big one stays");
        let others = data.others.expect("others");
        assert_eq!(others.count, 3);
        assert_eq!(others.size, 9, "dir_a + dir_b + file_c");
    }

    #[test]
    fn sub_percent_dir_goes_to_others() {
        let entries = vec![
            e("target", 68_858_598),
            e(".opencode", 54_891_593),
            e("screenshots", 406_369),
            e("src", 44_062),
            e(".git", 31_484),
            e("small", 8_790),
        ];
        let data = build_data(80 * 20, &entries, None);
        assert_eq!(data.items.len(), 2, "only target + .opencode");
        let others = data.others.expect("others");
        assert_eq!(others.count, 4);
        assert_eq!(others.size, 406_369 + 44_062 + 31_484 + 8_790);
    }

    #[test]
    fn to_cells_never_zero() {
        let (x, y, w, h) = to_cells((79.6, 12.9, 0.0, 0.0), 80, 13);
        assert!(w >= 1 && h >= 1);
        assert!(x < 80 && y < 13);
        assert!(x + w <= 80 && y + h <= 13);
    }

    #[test]
    fn chip_is_inset_from_edges() {
        let (x, y, cw, ch) = chip_rect(80, 13, 12).expect("chip");
        assert!(x + cw <= 79, "inset from right edge");
        assert!(y + ch <= 12, "inset from bottom edge");
        assert!(chip_rect(4, 1, 10).is_none());
    }
}
