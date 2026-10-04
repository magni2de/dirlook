use crate::app::App;
use crate::color;
use crate::screen::{Screen, DEFAULT_BG};
use crate::treemap;
use crate::width;

const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];
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

    let content_h = rows.saturating_sub(3);
    let tree_h = (content_h * 40 / 100).max(2);
    let map_h = content_h.saturating_sub(tree_h);
    let tree_top = 1usize;
    let sep_row = tree_top + tree_h;
    let map_top = sep_row + 1;
    let status_row = rows.saturating_sub(1);

    // Header
    let scan = if app.map_busy { SPINNER[(app.tick as usize) % 4] } else { " " };
    let header = format!("[dirlook — {}]  {}", app.focus.display(), scan);
    screen.put_str(
        0,
        0,
        &width::pad(&header, eff_cols, false),
        (130, 130, 140),
        DEFAULT_BG,
    );

    draw_tree(app, screen, tree_top, tree_h);
    draw_separator(app, screen, sep_row);
    draw_map(app, screen, map_top, map_h);

    // Status
    let selected = app.visible.get(app.selected);
    let sel_size = selected.map(|r| r.size).unwrap_or(0);
    let focus_size = app.map_subject().map(|n| n.size()).unwrap_or(0);
    let pct = if focus_size > 0 {
        sel_size as f64 / focus_size as f64 * 100.0
    } else {
        0.0
    };
    let sort = match app.sort {
        crate::app::SortMode::Name => "name",
        crate::app::SortMode::SizeDesc => "size↓",
        crate::app::SortMode::SizeAsc => "size↑",
    };
    let status = format!(
        " {} | {:.1}% of {} | sort:{} | ↑↓:move  →:expand  Enter:enter  ←:back  s:sort  /:legend  q:quit",
        color::human_size(sel_size),
        pct,
        color::human_size(focus_size),
        sort
    );
    screen.put_str(
        0,
        status_row,
        &width::pad(&status, eff_cols, false),
        (0, 200, 255),
        DEFAULT_BG,
    );

    if app.show_legend {
        draw_legend(screen);
    }

    let mut out = String::new();
    screen.flush(&mut out);
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
}

fn size_cell(known: bool, size: u64, done: u64, total: u64, tick: u64) -> String {
    if known {
        return color::human_size(size);
    }
    let spin = SPINNER[(tick as usize) % 4];
    let pct = if total > 0 {
        (done as f64 / total as f64 * 100.0).round() as u32
    } else {
        0
    };
    format!("{spin} {pct:>3}%")
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
        let size = size_cell(r.known, r.size, r.done, r.total, app.tick);
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
            let (sfr, sfg, sfb) = if r.known { (fr, fg_, fb) } else { (110, 140, 150) };
            screen.put_str(
                size_x,
                row,
                &width::pad(&size, size_w, true),
                (sfr, sfg, sfb),
                DEFAULT_BG,
            );
        }
    }
}

fn draw_separator(app: &App, screen: &mut Screen, row: usize) {
    let cols = screen.cols;
    let label = match app.map_subject() {
        Some(s) => {
            let known = if s.is_known() {
                color::human_size(s.size())
            } else {
                "scanning…".to_string()
            };
            format!(" map: {} ({}) ", s.path.display(), known)
        }
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
    match &app.map_layout {
        Some(layout) => treemap::draw(screen, 0, top, cols, height, layout),
        None => {
            let spin = SPINNER[(app.tick as usize) % 4];
            let msg = format!("{spin} computing map…");
            let cx = cols.saturating_sub(width::str_width(&msg)) / 2;
            let cy = top + height / 2;
            screen.put_str(cx, cy, &msg, (140, 140, 150), DEFAULT_BG);
        }
    }
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
    lines.push(vec![
        Seg::Swatch(treemap::UNKNOWN_COLOR),
        Seg::Text(" scanning / unknown".into(), text_fg),
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
