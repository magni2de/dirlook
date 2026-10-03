use crate::width;

pub const DEFAULT_FG: (u8, u8, u8) = (220, 220, 220);
pub const DEFAULT_BG: (u8, u8, u8) = (18, 18, 18);

/// Marker for the second cell of a wide character.
const CONT: char = '\u{0}';

#[derive(Clone, Copy, PartialEq)]
struct Cell {
    ch: char,
    fg: (u8, u8, u8),
    bg: (u8, u8, u8),
}

impl Cell {
    fn blank() -> Self {
        Cell {
            ch: ' ',
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
        }
    }
    fn sentinel() -> Self {
        // Never equal to a real cell, so the first frame redraws everything.
        Cell {
            ch: '\u{0}',
            fg: (1, 2, 3),
            bg: (4, 5, 6),
        }
    }
}

/// A double-buffered screen: we draw into `back`, then emit only the cells that
/// differ from `front` (what is currently on the terminal). This avoids the
/// full-screen clear/redraw flash — the terminal analog of swapping video RAM.
pub struct Screen {
    pub rows: usize,
    pub cols: usize,
    cell_w: usize,
    front: Vec<Cell>,
    back: Vec<Cell>,
    need_clear: bool,
}

impl Screen {
    pub fn new() -> Self {
        Screen {
            rows: 0,
            cols: 0,
            cell_w: 1,
            front: Vec::new(),
            back: Vec::new(),
            need_clear: true,
        }
    }

    /// `cols` is the number of logical characters; `cell_w` is how many physical
    /// terminal cells each occupies (1 normally, 2 in double-width mode).
    pub fn resize(&mut self, rows: usize, cols: usize, cell_w: usize) {
        if self.rows == rows && self.cols == cols && self.cell_w == cell_w {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.cell_w = cell_w.max(1);
        self.front = vec![Cell::blank(); rows * cols];
        self.back = vec![Cell::blank(); rows * cols];
        self.need_clear = true;
    }

    pub fn clear(&mut self) {
        for c in &mut self.back {
            *c = Cell::blank();
        }
    }

    pub fn put(&mut self, x: usize, y: usize, ch: char, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        if x < self.cols && y < self.rows {
            self.back[y * self.cols + x] = Cell { ch, fg, bg };
        }
    }

    pub fn put_str(&mut self, x: usize, y: usize, s: &str, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        if y >= self.rows {
            return;
        }
        let mut cx = x;
        for ch in s.chars() {
            let w = width::char_width(ch).max(1);
            if cx >= self.cols {
                break;
            }
            self.back[y * self.cols + cx] = Cell { ch, fg, bg };
            for k in 1..w {
                if cx + k < self.cols {
                    self.back[y * self.cols + cx + k] = Cell { ch: CONT, fg, bg };
                }
            }
            cx += w;
        }
    }

    pub fn fill_bg(&mut self, x: usize, y: usize, w: usize, h: usize, bg: (u8, u8, u8)) {
        for yy in y..(y + h).min(self.rows) {
            for xx in x..(x + w).min(self.cols) {
                self.back[yy * self.cols + xx].bg = bg;
            }
        }
    }

    /// Fill a rectangle completely (blank character + default fg + given bg),
    /// so underlying content does not show through an overlay panel.
    pub fn clear_rect(&mut self, x: usize, y: usize, w: usize, h: usize, bg: (u8, u8, u8)) {
        for yy in y..(y + h).min(self.rows) {
            for xx in x..(x + w).min(self.cols) {
                self.back[yy * self.cols + xx] = Cell {
                    ch: ' ',
                    fg: DEFAULT_FG,
                    bg,
                };
            }
        }
    }

    /// Emit the difference between `back` and `front` into `out`.
    pub fn flush(&mut self, out: &mut String) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        if self.need_clear {
            out.push_str("\x1b[2J");
            for c in &mut self.front {
                *c = Cell::sentinel();
            }
            self.need_clear = false;
        }

        let mut cur_fg: Option<(u8, u8, u8)> = None;
        let mut cur_bg: Option<(u8, u8, u8)> = None;
        let mut last: Option<(usize, usize)> = None;

        for y in 0..self.rows {
            for x in 0..self.cols {
                let i = y * self.cols + x;
                let b = self.back[i];
                if b.ch == CONT {
                    self.front[i] = b;
                    continue;
                }
                if b == self.front[i] {
                    continue;
                }
                if last != Some((x, y)) {
                    let col = 1 + x * self.cell_w;
                    out.push_str(&format!("\x1b[{};{}H", y + 1, col));
                }
                if cur_fg != Some(b.fg) {
                    out.push_str(&format!("\x1b[38;2;{};{};{}m", b.fg.0, b.fg.1, b.fg.2));
                    cur_fg = Some(b.fg);
                }
                if cur_bg != Some(b.bg) {
                    out.push_str(&format!("\x1b[48;2;{};{};{}m", b.bg.0, b.bg.1, b.bg.2));
                    cur_bg = Some(b.bg);
                }
                out.push(b.ch);
                self.front[i] = b;
                let w = width::char_width(b.ch).max(1);
                last = Some((x + w, y));
            }
        }
        out.push_str("\x1b[0m");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changed_cells_are_emitted() {
        let mut s = Screen::new();
        s.resize(2, 8, 1);
        s.clear();
        s.put_str(0, 0, "hello", (255, 0, 0), DEFAULT_BG);

        let mut out = String::new();
        s.flush(&mut out);
        assert!(out.contains("hello"));

        // Nothing changed → nothing to draw.
        let mut out2 = String::new();
        s.flush(&mut out2);
        assert_eq!(out2, "\x1b[0m", "second flush emitted {out2:?}");

        // One cell changed → only that cell is emitted.
        s.put(1, 0, 'E', (255, 0, 0), DEFAULT_BG);
        let mut out3 = String::new();
        s.flush(&mut out3);
        assert!(out3.contains('E'));
        assert!(!out3.contains("hello"), "full text re-emitted: {out3:?}");
    }
}
