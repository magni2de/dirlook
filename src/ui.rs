use crate::app::{App, Divider, LayoutMode, MARGIN_TOP, MARGIN_X};
use crate::color;
use crate::screen::{Screen, DEFAULT_BG};
use crate::treemap;
use crate::width;

const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];
const BRANCH_FG: (u8, u8, u8) = (92, 96, 110);
const CHROME_FG: (u8, u8, u8) = (130, 130, 140);
const SPIN_FG: (u8, u8, u8) = (255, 214, 0);

const PANEL_BG: (u8, u8, u8) = (28, 28, 42);
const PANEL_BORDER: (u8, u8, u8) = (120, 120, 140);
const HELP_TEXT: (u8, u8, u8) = (220, 220, 220);
const KEY_FG: (u8, u8, u8) = (255, 210, 120);
const HEAD_FG: (u8, u8, u8) = (120, 200, 255);
const PAD_X: usize = 4;
const PAD_Y: usize = 2;

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

    let regions = app.regions(rows, eff_cols);
    let status_row = rows.saturating_sub(1);
    let header_row = MARGIN_TOP.min(rows.saturating_sub(1));
    let mx = MARGIN_X.min(eff_cols.saturating_sub(1) / 2);
    let inner_w = eff_cols.saturating_sub(mx * 2).max(1);

    // Header — in the vertical layout the horizontal map separator is gone, so
    // the map subject/size moves to the right side of the header line.
    let spin = SPINNER[(app.tick as usize) % 4];
    let scan = if app.map_busy { spin } else { " " };
    let prefix = format!("[dirlook — {}]  ", app.focus.display());
    let prefix_w = width::str_width(&prefix);
    let left = format!("{prefix}{scan}");
    let spin_limit = if regions.mode == LayoutMode::Vertical {
        let right = format!(" {} ", map_label(app));
        let right_w = width::str_width(&right);
        if right_w + 1 < inner_w {
            let avail = inner_w - right_w;
            let left_t = width::truncate(&left, avail.saturating_sub(1));
            let mut line = width::pad(&left_t, avail, false);
            line.push_str(&right);
            screen.put_str(mx, header_row, &line, CHROME_FG, DEFAULT_BG);
            avail
        } else {
            screen.put_str(
                mx,
                header_row,
                &width::pad(&left, inner_w, false),
                CHROME_FG,
                DEFAULT_BG,
            );
            inner_w
        }
    } else {
        screen.put_str(
            mx,
            header_row,
            &width::pad(&left, inner_w, false),
            CHROME_FG,
            DEFAULT_BG,
        );
        inner_w
    };
    // Highlight the scan spinner in the header so active scanning stands out.
    if app.map_busy && prefix_w < spin_limit && mx + prefix_w < eff_cols {
        if let Some(c) = spin.chars().next() {
            screen.put(mx + prefix_w, header_row, c, SPIN_FG, DEFAULT_BG);
        }
    }

    draw_tree(
        app,
        screen,
        regions.tree.x,
        regions.tree.y,
        regions.tree.w,
        regions.tree.h,
    );
    match regions.divider {
        Divider::Vertical(col) => draw_vseparator(screen, col, regions.tree.y, regions.tree.h),
        Divider::Horizontal(row) => draw_separator(app, screen, row),
    }
    draw_map(
        app,
        screen,
        regions.map.x,
        regions.map.y,
        regions.map.w,
        regions.map.h,
    );

    // Status
    let selected = app.visible.get(app.selected);
    let sel_size = selected.map(|r| r.size).unwrap_or(0);
    let subject_size = app.map_subject().map(|n| n.size()).unwrap_or(0);
    // Share of the parent folder (fall back to the map subject when there is no
    // known parent, e.g. the scan root itself).
    let base_size = app.selected_parent_size().unwrap_or(subject_size);
    let pct = if base_size > 0 {
        sel_size as f64 / base_size as f64 * 100.0
    } else {
        0.0
    };
    let sort = match app.sort {
        crate::app::SortMode::Name => "name",
        crate::app::SortMode::SizeDesc => "size↓",
        crate::app::SortMode::SizeAsc => "size↑",
    };
    let status = if selected.map(|r| r.denied).unwrap_or(false) {
        " permission denied — allow access to this folder in System Settings > Privacy & Security > Files and Folders, then restart".to_string()
    } else {
        format!(
            " {:>9} | {:>5.1}% of {:>9} | sort:{} | ?:help",
            color::human_size(sel_size),
            pct,
            color::human_size(base_size),
            sort
        )
    };
    let status_w = width::str_width(&status);
    screen.put_str(
        mx,
        status_row,
        &width::pad(&status, inner_w, false),
        (0, 200, 255),
        DEFAULT_BG,
    );
    // Corner badge: the live auto-refresh mode (bottom-right, only when on).
    if app.watch {
        let badge = " ↻ watching";
        let bw = width::str_width(&badge);
        if status_w + bw + 2 <= inner_w {
            screen.put_str(mx + inner_w - bw, status_row, badge, (110, 190, 170), DEFAULT_BG);
        }
    }

    if app.show_help {
        draw_help(screen);
    }

    let mut out = String::new();
    screen.flush(&mut out);
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
}

fn map_label(app: &App) -> String {
    match app.map_subject() {
        Some(s) => {
            let known = if s.is_denied() {
                "permission denied".to_string()
            } else if s.is_known() {
                color::human_size(s.size())
            } else {
                "scanning…".to_string()
            };
            format!("map: {} ({})", s.path.display(), known)
        }
        None => "map".to_string(),
    }
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

fn draw_tree(app: &App, screen: &mut Screen, x: usize, top: usize, width: usize, height: usize) {
    if width == 0 || height == 0 {
        return;
    }
    let size_w: usize = 12;
    let name_w = width.saturating_sub(size_w + 2).max(1);

    let mut start = 0;
    if app.visible.len() > height {
        start = app.selected.saturating_sub(height / 2);
        if start + height > app.visible.len() {
            start = app.visible.len() - height;
        }
    }
    let end = (start + height).min(app.visible.len());

    for i in start..end {
        let r = &app.visible[i];
        let row = top + (i - start);
        let is_selected = i == app.selected;
        let size = if r.denied {
            "denied".to_string()
        } else {
            size_cell(r.known, r.size, r.done, r.total, app.tick)
        };
        let size_x = x + name_w + 2;

        if r.is_parent {
            let line = format!(
                "{}  {}",
                width::pad("..", name_w, false),
                width::pad(&size, size_w, true)
            );
            let fg = if is_selected { (20, 20, 20) } else { (120, 140, 220) };
            let bg = if is_selected { (200, 200, 200) } else { DEFAULT_BG };
            screen.put_str(x, row, &line, fg, bg);
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
            screen.put_str(x, row, &line, (20, 20, 20), (200, 200, 200));
        } else {
            let pw = width::str_width(&r.prefix);
            screen.put_str(x, row, &r.prefix, BRANCH_FG, DEFAULT_BG);
            let rest_raw = format!("{}{}", arrow, r.name);
            let rest = width::truncate(&rest_raw, name_w.saturating_sub(pw));
            screen.put_str(x + pw, row, &rest, (fr, fg_, fb), DEFAULT_BG);
            let (sfr, sfg, sfb) = if r.denied {
                (230, 120, 80)
            } else if r.known {
                (fr, fg_, fb)
            } else {
                SPIN_FG
            };
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

fn draw_vseparator(screen: &mut Screen, col: usize, top: usize, height: usize) {
    for row in top..top + height {
        screen.put(col, row, '│', CHROME_FG, DEFAULT_BG);
    }
}

fn draw_separator(app: &App, screen: &mut Screen, row: usize) {
    let cols = screen.cols;
    let label = format!(" {} ", map_label(app));
    let head = format!("──{}", label);
    let used = width::str_width(&head);
    let fill = cols.saturating_sub(used);
    let mut line = head;
    line.push_str(&"─".repeat(fill));
    let line = width::truncate(&line, cols);
    screen.put_str(0, row, &line, CHROME_FG, DEFAULT_BG);
}

fn draw_map(app: &App, screen: &mut Screen, x: usize, top: usize, width: usize, height: usize) {
    if width == 0 || height == 0 {
        return;
    }
    match &app.map_layout {
        Some(layout) => treemap::draw(screen, x, top, width, height, layout),
        None => {
            let spin = SPINNER[(app.tick as usize) % 4];
            let msg = format!("{spin} computing map…");
            let cx = x + width.saturating_sub(width::str_width(&msg)) / 2;
            let cy = top + height / 2;
            screen.put_str(cx, cy, &msg, SPIN_FG, DEFAULT_BG);
        }
    }
}

enum Seg {
    Text(String, (u8, u8, u8)),
    Swatch((u8, u8, u8)),
}

fn draw_help(screen: &mut Screen) {
    let mut left = vec![vec![Seg::Text("Keys".into(), HEAD_FG)], vec![]];
    left.extend(key_lines());
    let mut right = vec![vec![Seg::Text("Colors".into(), HEAD_FG)], vec![]];
    right.extend(color_lines());

    let rows = screen.rows;
    let cols = screen.cols;

    let lw = lines_width(&left);
    let rw = lines_width(&right);
    let body_h = left.len().max(right.len());

    // One shared frame, two columns side by side, with a thin rule between.
    let pad_l = 3;
    let pad_r = 3; // the colours column sits a few spaces clear of the rule
    let box_w = lw + pad_l + 1 + pad_r + rw + 2 * PAD_X;
    let box_h = body_h + 2 * PAD_Y;
    if box_w <= cols && box_h + 2 <= rows {
        let x0 = (cols - box_w) / 2;
        let y0 = (rows - box_h) / 2;
        draw_panel(screen, x0, y0, box_w, box_h, "Help");
        let cx = x0 + PAD_X;
        let cy = y0 + PAD_Y;
        draw_segs(screen, cx, cy, &left);
        let dx = cx + lw + pad_l;
        draw_segs(screen, dx + 1 + pad_r, cy, &right);
        // Thin rule between the two columns.
        for yy in cy..cy + body_h {
            screen.put(dx, yy, '│', PANEL_BORDER, PANEL_BG);
        }
        return;
    }

    // Narrow terminal: same single frame, columns stacked, with a blank line
    // after the rule so the second block is not glued to it.
    let gap_rows = 2; // rule + blank line
    let avail = rows.saturating_sub(2 * PAD_Y + 2).max(1);
    let mut left = left;
    let mut right = right;
    if left.len() + gap_rows + right.len() > avail {
        let mut excess = left.len() + gap_rows + right.len() - avail;
        let cut = right.len().saturating_sub(2).min(excess);
        right.truncate(right.len() - cut);
        excess -= cut;
        let cut = left.len().saturating_sub(2).min(excess);
        left.truncate(left.len() - cut);
    }
    let inner_w = lines_width(&left).max(lines_width(&right));
    let box_w = (inner_w + 2 * PAD_X).min(cols);
    let box_h = left.len() + gap_rows + right.len() + 2 * PAD_Y;
    let x0 = cols.saturating_sub(box_w) / 2;
    let y0 = rows.saturating_sub(box_h) / 2;
    draw_panel(screen, x0, y0, box_w, box_h, "Help");
    let cx = x0 + PAD_X;
    let mut cy = y0 + PAD_Y;
    draw_segs(screen, cx, cy, &left);
    cy += left.len();
    for xx in cx..cx + box_w.saturating_sub(2 * PAD_X) {
        screen.put(xx, cy, '─', PANEL_BORDER, PANEL_BG);
    }
    cy += gap_rows;
    draw_segs(screen, cx, cy, &right);
}

/// Left block: the hotkey table.
fn key_lines() -> Vec<Vec<Seg>> {
    const KEYS: &[(&str, &str)] = &[
        ("↑ / ↓, k / j", "Move the selection"),
        ("→", "Expand / collapse a folder"),
        ("Enter", "Enter the selected folder"),
        ("←, Backspace", "Collapse / go to parent"),
        ("s", "Cycle sort order"),
        ("[ / ]", "Move the tree/map divider"),
        ("m", "Toggle layout"),
        ("r", "Refresh the current folder"),
        ("w", "Toggle auto-refresh"),
        ("? /", "Show this help"),
        ("q", "Quit"),
    ];
    let kw = KEYS.iter().map(|(k, _)| width::str_width(k)).max().unwrap_or(0);
    let mut lines = Vec::with_capacity(KEYS.len());
    for (k, a) in KEYS {
        let pad = kw - width::str_width(k);
        lines.push(vec![
            Seg::Text((*k).to_string(), KEY_FG),
            Seg::Text(format!("{}   ", " ".repeat(pad)), HELP_TEXT),
            Seg::Text((*a).to_string(), HELP_TEXT),
        ]);
    }
    lines
}

/// Right block: the color legend by file type.
fn color_lines() -> Vec<Vec<Seg>> {
    let mut lines: Vec<Vec<Seg>> = Vec::new();
    lines.push(vec![Seg::Text(
        "Color = file type (tree and map)".into(),
        HELP_TEXT,
    )]);
    lines.push(vec![]);
    lines.push(vec![
        Seg::Swatch(color::DIR_COLOR),
        Seg::Text(" directories".into(), HELP_TEXT),
    ]);
    for cat in color::CATEGORIES {
        let sample: Vec<&str> = cat.exts.iter().take(4).copied().collect();
        lines.push(vec![
            Seg::Swatch(cat.color),
            Seg::Text(format!(" {} ({})", cat.name, sample.join(", ")), HELP_TEXT),
        ]);
    }
    lines.push(vec![
        Seg::Swatch(treemap::OTHER_COLOR),
        Seg::Text(" ⋯ others".into(), HELP_TEXT),
    ]);
    lines.push(vec![
        Seg::Swatch(treemap::UNKNOWN_COLOR),
        Seg::Text(" scanning / unknown".into(), HELP_TEXT),
    ]);
    lines
}

fn segs_width(segs: &[Seg]) -> usize {
    segs.iter()
        .map(|s| match s {
            Seg::Text(t, _) => width::str_width(t),
            Seg::Swatch(_) => 2,
        })
        .sum()
}

fn lines_width(lines: &[Vec<Seg>]) -> usize {
    lines.iter().map(|l| segs_width(l)).max().unwrap_or(0)
}

fn draw_segs(screen: &mut Screen, x: usize, y: usize, lines: &[Vec<Seg>]) {
    for (i, segs) in lines.iter().enumerate() {
        let mut cx = x;
        for seg in segs {
            match seg {
                Seg::Swatch(c) => {
                    screen.fill_bg(cx, y + i, 2, 1, *c);
                    cx += 2;
                }
                Seg::Text(t, fg) => {
                    screen.put_str(cx, y + i, t, *fg, PANEL_BG);
                    cx += width::str_width(t);
                }
            }
        }
    }
}

/// A bordered panel with a title in the top border.
fn draw_panel(screen: &mut Screen, x: usize, y: usize, w: usize, h: usize, title: &str) {
    if w < 4 || h < 2 {
        return;
    }
    screen.clear_rect(x, y, w, h, PANEL_BG);

    let head = format!("─ {title} ");
    let mut top = String::from("┌");
    top.push_str(&head);
    let fill = w.saturating_sub(width::str_width(&top) + 1);
    top.push_str(&"─".repeat(fill));
    top.push('┐');
    screen.put_str(x, y, &width::truncate(&top, w), PANEL_BORDER, PANEL_BG);

    let bottom = format!("└{}┘", "─".repeat(w.saturating_sub(2)));
    screen.put_str(x, y + h - 1, &bottom, PANEL_BORDER, PANEL_BG);

    for yy in y + 1..y + h - 1 {
        screen.put(x, yy, '│', PANEL_BORDER, PANEL_BG);
        screen.put(x + w - 1, yy, '│', PANEL_BORDER, PANEL_BG);
    }
}
