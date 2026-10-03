use std::sync::atomic::Ordering;

use crate::app::App;
use crate::color;
use crate::node::Node;
use crate::screen::{Screen, DEFAULT_BG, DEFAULT_FG};
use crate::treemap;
use crate::width;

const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];
const BAR_FG: (u8, u8, u8) = (165, 165, 165);
const BRANCH_FG: (u8, u8, u8) = (92, 96, 110);

pub fn render(app: &App, screen: &mut Screen) {
    let (rows_u16, cols_u16) = crate::term::terminal_size();
    let rows = rows_u16 as usize;
    let cols = cols_u16 as usize;
    let cell = app.cell_width.max(1) as usize;
    let eff_cols = cols / cell;
    if rows == 0 || eff_cols == 0 {
        return;
    }

    screen.resize(rows, eff_cols, cell);
    screen.clear();

    if app.scanning.is_some() {
        draw_scan(app, screen);
    } else {
        draw_main(app, screen);
    }

    let mut out = String::new();
    screen.flush(&mut out);
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
}

fn draw_main(app: &App, screen: &mut Screen) {
    let rows = screen.rows;
    let cols = screen.cols;
    if rows < 6 {
        return;
    }

    let body = rows - 3; // rows minus header, separator, status
    let tree_h = (body * 40 / 100).max(2);
    let map_h = body.saturating_sub(tree_h);
    if map_h < 1 {
        return;
    }
    let tree_top = 1usize;
    let sep_row = tree_top + tree_h;
    let map_top = sep_row + 1;

    let path_str = app.focus.display().to_string();
    let header = format!("[dirlook — {}]", path_str);
    screen.put_str(0, 0, &width::pad(&header, cols, false), (130, 130, 140), DEFAULT_BG);

    draw_tree(app, screen, tree_top, tree_h);
    draw_separator(app, screen, sep_row);
    draw_map(app, screen, map_top, map_h);

    let total = app.root.as_ref().map(|r| r.size).unwrap_or(0);
    let selected_size = app
        .visible
        .get(app.selected)
        .map(|r| if r.is_parent { 0 } else { r.size })
        .unwrap_or(0);
    let pct = if total > 0 {
        selected_size as f64 / total as f64 * 100.0
    } else {
        0.0
    };
    let sort_arrow = if app.sort_desc { "↓" } else { "↑" };
    let status = format!(
        " {} | {:.1}% of {} | sort:{} | ↑↓:move  →:expand  Enter:enter  ←:back  s:sort  /:legend  q:quit",
        color::human_size(selected_size),
        pct,
        color::human_size(total),
        sort_arrow
    );
    screen.put_str(
        0,
        rows - 1,
        &width::pad(&status, cols, false),
        (0, 200, 255),
        DEFAULT_BG,
    );

    if app.show_legend {
        draw_legend(screen);
    }
}

fn draw_scan(app: &App, screen: &mut Screen) {
    let state = match &app.scanning {
        Some(s) => s,
        None => return,
    };
    let rows = screen.rows;
    let cols = screen.cols;
    if rows < 3 {
        return;
    }

    let st = &state.status;
    let dirs = st.dirs.load(Ordering::Relaxed);
    let files = st.files.load(Ordering::Relaxed);
    let bytes = st.bytes.load(Ordering::Relaxed);
    let secs = state.started.elapsed().as_secs();
    let spin = SPINNER[(state.tick as usize) % SPINNER.len()];
    let bar_w = 20usize;

    // Root progress + the active child's smooth fraction come from the chain.
    let chain = st.chain.lock().map(|c| c.clone()).unwrap_or_default();
    let nodes: Vec<(u64, u64)> = chain
        .iter()
        .map(|d| (d.total, d.done.load(Ordering::Relaxed)))
        .collect();
    let fracs = chain_fractions(&nodes);
    let global = fracs.first().copied().unwrap_or(0.0);
    let active = chain
        .first()
        .map(|d| d.done.load(Ordering::Relaxed) as usize)
        .unwrap_or(0);
    let active_frac = fracs.get(1).copied().unwrap_or(0.0);
    let active_partial = chain
        .get(1)
        .map(|d| d.partial.load(Ordering::Relaxed))
        .unwrap_or(0);

    // Row 0: title (left) + global bar (right-aligned, white).
    let title = format!("Scanning  {}", state.target.display());
    let bar_len = bar_w + 2;
    let bar_x = cols.saturating_sub(bar_len);
    let gmeta = format!("overall {:>3}%", (global * 100.0).round() as u32);
    let gmeta_x = bar_x.saturating_sub(2 + width::str_width(&gmeta));
    let title = width::truncate(&title, gmeta_x.saturating_sub(1));
    screen.put_str(0, 0, &title, (0, 200, 255), DEFAULT_BG);
    screen.put_str(gmeta_x, 0, &gmeta, (0, 200, 255), DEFAULT_BG);
    screen.put_str(bar_x, 0, &bar(global, bar_w), BAR_FG, DEFAULT_BG);

    // Row 1: counters.
    if rows >= 2 {
        let counters = format!(
            " {}  dirs: {}   files: {}   size: {}   {:02}:{:02}",
            spin,
            dirs,
            files,
            color::human_size(bytes),
            secs / 60,
            secs % 60
        );
        screen.put_str(0, 1, &width::pad(&counters, cols, false), DEFAULT_FG, DEFAULT_BG);
    }

    // Rows 2..: still-running and pending children. Completed ones (before
    // `active`) have already vanished.
    let children = st.root_children.lock().map(|c| c.clone()).unwrap_or_default();
    let avail = rows.saturating_sub(2);
    if avail == 0 {
        return;
    }
    let start = active.min(children.len());

    let mut r = 2usize;
    for (j, child) in children.iter().enumerate().skip(start).take(avail) {
        let (frac, pct, size, fg) = if j == active {
            let sz = if active_partial > 0 {
                color::human_size(active_partial)
            } else {
                String::new()
            };
            (active_frac, (active_frac * 100.0).round() as u32, sz, (0, 200, 255))
        } else {
            (0.0, 0u32, String::new(), (120, 120, 130))
        };
        let meta = format!("{:>9}  {:>3}%", size, pct);
        let meta_x = bar_x.saturating_sub(2 + width::str_width(&meta));
        let name = width::truncate(&child.name, meta_x.saturating_sub(1));
        screen.put_str(0, r, &name, fg, DEFAULT_BG);
        screen.put_str(meta_x, r, &meta, fg, DEFAULT_BG);
        screen.put_str(bar_x, r, &bar(frac, bar_w), BAR_FG, DEFAULT_BG);
        r += 1;
    }
}

fn bar(frac: f64, width: usize) -> String {
    let filled = ((frac.clamp(0.0, 1.0)) * width as f64).round() as usize;
    let filled = filled.min(width);
    let mut s = String::from("[");
    for i in 0..width {
        s.push(if i < filled { '▒' } else { '░' });
    }
    s.push(']');
    s
}

/// Smooth progress fractions for a root→current chain of `(total, done)` counts.
/// `frac[i] = (done[i] + frac[i+1]) / total[i]`, so a deep, long-running subtree
/// still advances its ancestors' bars.
fn chain_fractions(nodes: &[(u64, u64)]) -> Vec<f64> {
    let mut fracs = vec![0f64; nodes.len()];
    let mut child = 0f64;
    for i in (0..nodes.len()).rev() {
        let (t, d) = nodes[i];
        let f = if t > 0 {
            ((d as f64 + child) / t as f64).min(1.0)
        } else {
            0.0
        };
        fracs[i] = f;
        child = f;
    }
    fracs
}

enum Seg {
    Text(String, (u8, u8, u8)),
    Swatch((u8, u8, u8)),
}

fn draw_legend(screen: &mut Screen) {
    let rows = screen.rows;
    let cols = screen.cols;
    let panel_bg = (28, 28, 42);
    let border_fg = (120, 120, 140);
    let text_fg = (220, 220, 220);

    let mut lines: Vec<Vec<Seg>> = Vec::new();
    lines.push(vec![Seg::Text("Legend".into(), (120, 200, 255))]);
    lines.push(vec![]);
    lines.push(vec![Seg::Text(
        "Color = file type (tree and map)".into(),
        text_fg,
    )]);
    lines.push(vec![
        Seg::Swatch(color::DIR_COLOR),
        Seg::Text(" directories".into(), text_fg),
    ]);
    for cat in color::CATEGORIES {
        let sample: Vec<&str> = cat.exts.iter().take(4).copied().collect();
        lines.push(vec![
            Seg::Swatch(cat.color),
            Seg::Text(format!(" {} ({})", cat.name, sample.join(", ")), text_fg),
        ]);
    }
    lines.push(vec![
        Seg::Swatch(treemap::OTHER_COLOR),
        Seg::Text(" ⋯ others".into(), text_fg),
    ]);

    let plain_w = |segs: &[Seg]| -> usize {
        segs.iter()
            .map(|s| match s {
                Seg::Text(t, _) => width::str_width(t),
                Seg::Swatch(_) => 2,
            })
            .sum()
    };
    let max_plain = lines.iter().map(|l| plain_w(l)).max().unwrap_or(0);
    let box_w = (max_plain + 4).min(cols.saturating_sub(2));
    let box_h = lines.len() + 2;
    if box_w < 10 || rows < box_h + 1 {
        return;
    }
    let x0 = (cols - box_w) / 2;
    let y0 = (rows - box_h) / 2;

    screen.clear_rect(x0, y0, box_w, box_h, panel_bg);
    // Borders
    for xx in x0..x0 + box_w {
        screen.put(xx, y0, '─', border_fg, panel_bg);
        screen.put(xx, y0 + box_h - 1, '─', border_fg, panel_bg);
    }
    for yy in y0..y0 + box_h {
        screen.put(x0, yy, '│', border_fg, panel_bg);
        screen.put(x0 + box_w - 1, yy, '│', border_fg, panel_bg);
    }
    screen.put(x0, y0, '┌', border_fg, panel_bg);
    screen.put(x0 + box_w - 1, y0, '┐', border_fg, panel_bg);
    screen.put(x0, y0 + box_h - 1, '└', border_fg, panel_bg);
    screen.put(x0 + box_w - 1, y0 + box_h - 1, '┘', border_fg, panel_bg);

    // Content
    for (i, segs) in lines.iter().enumerate() {
        let mut cx = x0 + 2;
        let cy = y0 + 1 + i;
        for seg in segs {
            match seg {
                Seg::Swatch(c) => {
                    screen.fill_bg(cx, cy, 2, 1, *c);
                    cx += 2;
                }
                Seg::Text(t, fg) => {
                    screen.put_str(cx, cy, t, *fg, panel_bg);
                    cx += width::str_width(t);
                }
            }
        }
    }
}

fn draw_tree(app: &App, screen: &mut Screen, top: usize, height: usize) {
    let cols = screen.cols;
    let size_w: usize = 12;
    let name_w = cols.saturating_sub(size_w + 2).max(1);

    let mut start = 0;
    if app.visible.len() > height && app.selected >= height {
        start = app.selected - height / 2 + 1;
        if start + height > app.visible.len() {
            start = app.visible.len().saturating_sub(height);
        }
    }
    let end = (start + height).min(app.visible.len());

    for i in start..end {
        let r = &app.visible[i];
        let row = top + (i - start);
        let is_selected = i == app.selected;
        let size = color::human_size(r.size);
        let size_x = name_w + 2;

        if r.is_parent {
            let line = format!(
                "{}  {}",
                width::pad("..", name_w, false),
                width::pad(&size, size_w, true)
            );
            let fg = if is_selected { (20, 20, 20) } else { (120, 140, 220) };
            let bg = if is_selected { (200, 200, 200) } else { DEFAULT_BG };
            screen.put_str(0, row, &line, fg, bg);
            continue;
        }

        let arrow = if r.is_dir {
            if r.is_expanded {
                "▾ "
            } else {
                "▸ "
            }
        } else {
            "  "
        };
        let (fr, fg_, fb) = if r.is_dir {
            color::DIR_COLOR
        } else {
            color::ext_color(&r.name, false)
        };

        if is_selected {
            let label = format!("{}{}{}", r.prefix, arrow, r.name);
            let name = width::truncate(&label, name_w);
            let line = format!(
                "{}  {}",
                width::pad(&name, name_w, false),
                width::pad(&size, size_w, true)
            );
            screen.put_str(0, row, &line, (20, 20, 20), (200, 200, 200));
        } else {
            let pw = width::str_width(&r.prefix);
            screen.put_str(0, row, &r.prefix, BRANCH_FG, DEFAULT_BG);
            let rest_raw = format!("{}{}", arrow, r.name);
            let rest = width::truncate(&rest_raw, name_w.saturating_sub(pw));
            screen.put_str(pw, row, &rest, (fr, fg_, fb), DEFAULT_BG);
            screen.put_str(
                size_x,
                row,
                &width::pad(&size, size_w, true),
                (fr, fg_, fb),
                DEFAULT_BG,
            );
        }
    }
}

fn draw_separator(app: &App, screen: &mut Screen, row: usize) {
    let cols = screen.cols;
    let label = match app.map_subject() {
        Some(s) => format!(
            " map: {} ({}) ",
            s.path.display(),
            color::human_size(s.size)
        ),
        None => " map ".to_string(),
    };
    let head = format!("──{}", label);
    let used = width::str_width(&head);
    let fill = cols.saturating_sub(used);
    let mut line = head;
    line.push_str(&"─".repeat(fill));
    let line = width::truncate(&line, cols);
    screen.put_str(0, row, &line, (130, 130, 140), DEFAULT_BG);
}

fn draw_map(app: &App, screen: &mut Screen, top: usize, height: usize) {
    let cols = screen.cols;
    let subject = match app.map_subject() {
        Some(s) => s,
        None => {
            treemap::draw(screen, 0, top, cols, height, &treemap::Data::default());
            return;
        }
    };

    let mut children: Vec<&Node> = subject.children.iter().collect();
    children.sort_by(|a, b| b.size.cmp(&a.size));

    let hl_path = app.highlight_path();
    let mut hl_idx: Option<usize> = None;
    let mut entries: Vec<(String, u64, (u8, u8, u8))> = Vec::with_capacity(children.len());
    for (i, c) in children.iter().enumerate() {
        if hl_path.as_deref() == Some(c.path.as_path()) {
            hl_idx = Some(i);
        }
        entries.push((c.name.clone(), c.size, color::ext_color(&c.name, c.is_dir)));
    }

    let cells = cols.saturating_mul(height);
    let data = treemap::build_data(cells, &entries, hl_idx);
    treemap::draw(screen, 0, top, cols, height, &data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_fractions_propagate() {
        // root: 2 direct children, 1 done; current child: 4 children, 1 done
        let f = chain_fractions(&[(2, 1), (4, 1)]);
        assert!((f[1] - 0.25).abs() < 1e-9);
        assert!((f[0] - 0.625).abs() < 1e-9, "got {}", f[0]);
    }

    #[test]
    fn fractions_handle_zero_total() {
        let f = chain_fractions(&[(0, 0), (3, 3)]);
        assert_eq!(f[0], 0.0);
        assert!((f[1] - 1.0).abs() < 1e-9);
    }
}
