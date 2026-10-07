use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crate::color;
use crate::key::KeyCode;
use crate::node::Node;
use crate::scanner::{self, Engine, Index};
use crate::treemap::{self, MapEntry};

/// Children count up to which the map is laid out synchronously (instant).
const SYNC_LIMIT: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortMode {
    Name,
    SizeDesc,
    SizeAsc,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayoutMode {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum Divider {
    Vertical(usize),
    Horizontal(usize),
}

#[derive(Debug, Clone, Copy)]
pub struct Regions {
    pub mode: LayoutMode,
    pub tree: Rect,
    pub map: Rect,
    pub divider: Divider,
}

#[derive(Debug, Clone)]
pub struct VisibleRow {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub known: bool,
    pub denied: bool,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub is_parent: bool,
    pub prefix: String,
    pub done: u64,
    pub total: u64,
}

impl VisibleRow {
    pub fn depth(&self) -> usize {
        self.prefix.chars().count() / 3
    }
}

pub struct MapRequest {
    pub generation: u64,
    pub subject: PathBuf,
    pub sig: u64,
    pub w: usize,
    pub h: usize,
    pub entries: Vec<MapEntry>,
}

pub struct MapResult {
    pub generation: u64,
    pub subject: PathBuf,
    pub sig: u64,
    pub layout: treemap::Layout,
}

pub struct App {
    pub root: Option<Arc<Node>>,
    pub index: Index,
    engine: Option<Engine>,
    pub visible: Vec<VisibleRow>,
    pub selected: usize,
    pub focus: PathBuf,
    pub expanded: HashSet<PathBuf>,
    pub sort: SortMode,
    pub quit: bool,
    pub cell_width: u32,
    pub show_legend: bool,
    pub tick: u64,
    pub layout: LayoutMode,
    pub split_v: usize,
    pub split_h: usize,

    map_tx: Sender<MapRequest>,
    map_rx: Receiver<MapResult>,
    map_gen: u64,
    map_key: Option<(PathBuf, u64)>,
    map_cache: HashMap<PathBuf, (u64, treemap::Layout)>,
    pub map_layout: Option<treemap::Layout>,
    map_last_send: Instant,
    pub map_busy: bool,
}

impl App {
    pub fn new(root_path: PathBuf) -> Self {
        let engine = scanner::bootstrap(root_path.clone());
        let root = Arc::clone(&engine.root);
        let index = Arc::clone(&engine.index);
        let mut expanded = HashSet::new();
        expanded.insert(root.path.clone());

        let (map_tx, req_rx) = mpsc::channel::<MapRequest>();
        let (res_tx, map_rx) = mpsc::channel::<MapResult>();
        std::thread::spawn(move || {
            while let Ok(req) = req_rx.recv() {
                let layout = treemap::compute(req.w, req.h, &req.entries);
                if res_tx
                    .send(MapResult {
                        generation: req.generation,
                        subject: req.subject,
                        sig: req.sig,
                        layout,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut app = Self {
            root: Some(Arc::clone(&root)),
            index,
            engine: Some(engine),
            visible: Vec::new(),
            selected: 0,
            focus: root.path.clone(),
            expanded,
            sort: SortMode::Name,
            quit: false,
            cell_width: 1,
            show_legend: false,
            tick: 0,
            layout: LayoutMode::Vertical,
            split_v: 30,
            split_h: 40,
            map_tx,
            map_rx,
            map_gen: 0,
            map_key: None,
            map_cache: HashMap::new(),
            map_layout: None,
            map_last_send: Instant::now(),
            map_busy: false,
        };
        // Wait briefly for the root's first listing so the first frame is full
        // (list of children + first map) instead of flashing empty then filling.
        let deadline = Instant::now() + Duration::from_secs(1);
        while !root.listed.load(Ordering::Relaxed) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        app.refresh();
        app
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    fn boost_focus(&self) {
        if let Some(e) = &self.engine {
            e.boost(&self.focus, true);
        }
    }

    fn node_by_path(&self, path: &std::path::Path) -> Option<Arc<Node>> {
        if let Some(n) = self.index.lock().unwrap().get(path) {
            return Some(Arc::clone(n));
        }
        self.root.as_ref().and_then(|r| Node::find(r, path))
    }

    fn subject_node(&self) -> Option<Arc<Node>> {
        self.root.as_ref()?;
        let row = self.visible.get(self.selected)?;
        if row.is_parent {
            return self.node_by_path(&self.focus);
        }
        let node = self.node_by_path(&row.path)?;
        if node.is_dir {
            Some(node)
        } else {
            let parent = node.path.parent()?;
            self.node_by_path(parent)
        }
    }

    pub fn map_subject(&self) -> Option<Arc<Node>> {
        self.subject_node()
    }

    pub fn highlight_path(&self) -> Option<PathBuf> {
        let row = self.visible.get(self.selected)?;
        if row.is_parent {
            None
        } else {
            Some(row.path.clone())
        }
    }

    /// Compute the tree/map geometry for the current terminal size.
    ///
    /// Kept in one place so `ui::render` and the off-thread map builder in
    /// `pump` always agree on the map's dimensions.
    pub fn regions(&self, rows: usize, cols: usize) -> Regions {
        match self.layout {
            LayoutMode::Vertical => {
                let content_h = rows.saturating_sub(2);
                let max_tree = cols.saturating_sub(2).max(1);
                let tree_w = (cols * self.split_v / 100).clamp(1, max_tree);
                let map_x = tree_w + 1;
                let map_w = cols.saturating_sub(map_x);
                Regions {
                    mode: LayoutMode::Vertical,
                    tree: Rect {
                        x: 0,
                        y: 1,
                        w: tree_w,
                        h: content_h,
                    },
                    map: Rect {
                        x: map_x,
                        y: 1,
                        w: map_w,
                        h: content_h,
                    },
                    divider: Divider::Vertical(tree_w),
                }
            }
            LayoutMode::Horizontal => {
                let content_h = rows.saturating_sub(3);
                let tree_h = (content_h * self.split_h / 100).max(2);
                let map_h = content_h.saturating_sub(tree_h);
                let sep_row = 1 + tree_h;
                Regions {
                    mode: LayoutMode::Horizontal,
                    tree: Rect {
                        x: 0,
                        y: 1,
                        w: cols,
                        h: tree_h,
                    },
                    map: Rect {
                        x: 0,
                        y: sep_row + 1,
                        w: cols,
                        h: map_h,
                    },
                    divider: Divider::Horizontal(sep_row),
                }
            }
        }
    }

    fn split_mut(&mut self) -> &mut usize {
        match self.layout {
            LayoutMode::Vertical => &mut self.split_v,
            LayoutMode::Horizontal => &mut self.split_h,
        }
    }

    fn force_map_rebuild(&mut self) {
        let past = Instant::now()
            .checked_sub(Duration::from_millis(500))
            .unwrap_or_else(Instant::now);
        if self.map_last_send > past {
            self.map_last_send = past;
        }
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('/') | KeyCode::Char('?') => self.show_legend = !self.show_legend,
            KeyCode::Escape => self.show_legend = false,
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.visible.is_empty() && self.selected < self.visible.len() - 1 {
                    self.selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Right => {
                if let Some(row) = self.visible.get(self.selected).cloned() {
                    if row.is_parent {
                        self.up_or_ascend();
                    } else if row.is_dir {
                        if !row.is_expanded {
                            self.expanded.insert(row.path.clone());
                            self.refresh();
                        } else if let Some(neighbor) = self.visible.get(self.selected + 1) {
                            if neighbor.depth() == row.depth() + 1 && !neighbor.is_parent {
                                self.selected += 1;
                            }
                        }
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(row) = self.visible.get(self.selected).cloned() {
                    if row.is_parent {
                        self.up_or_ascend();
                    } else if row.is_dir {
                        self.enter_dir(&row.path);
                    }
                }
            }
            KeyCode::Left | KeyCode::Backspace => {
                if let Some(row) = self.visible.get(self.selected).cloned() {
                    if row.is_parent {
                        // nothing
                    } else if row.is_dir && row.is_expanded {
                        self.expanded.remove(&row.path);
                        self.refresh();
                    } else {
                        self.move_to_parent_row();
                    }
                }
            }
            KeyCode::Char('s') => {
                self.sort = match self.sort {
                    SortMode::Name => SortMode::SizeDesc,
                    SortMode::SizeDesc => SortMode::SizeAsc,
                    SortMode::SizeAsc => SortMode::Name,
                };
                self.refresh();
            }
            KeyCode::Char('m') => {
                self.layout = match self.layout {
                    LayoutMode::Vertical => LayoutMode::Horizontal,
                    LayoutMode::Horizontal => LayoutMode::Vertical,
                };
                self.force_map_rebuild();
            }
            KeyCode::Char(']') => {
                let s = self.split_mut();
                *s = (*s + 5).min(85);
                self.force_map_rebuild();
            }
            KeyCode::Char('[') => {
                let s = self.split_mut();
                *s = s.saturating_sub(5).max(15);
                self.force_map_rebuild();
            }
            _ => {}
        }
    }

    fn enter_dir(&mut self, path: &std::path::Path) {
        self.focus = path.to_path_buf();
        self.expanded.insert(path.to_path_buf());
        self.boost_focus();
        self.refresh();
        self.selected = self
            .visible
            .iter()
            .position(|r| !r.is_parent && r.path == path)
            .unwrap_or(0);
    }

    fn up_or_ascend(&mut self) {
        let root_path = self.root.as_ref().map(|r| r.path.clone());
        if Some(self.focus.clone()) == root_path {
            if let Some(parent) = self.focus.parent().map(|p| p.to_path_buf()) {
                self.rebootstrap(parent);
            }
        } else if let Some(parent) = self.focus.parent().map(|p| p.to_path_buf()) {
            let from = self.focus.clone();
            self.focus = parent;
            self.boost_focus();
            self.refresh();
            self.selected = self
                .visible
                .iter()
                .position(|r| !r.is_parent && r.path == from)
                .or_else(|| self.visible.iter().position(|r| !r.is_parent))
                .unwrap_or(0);
        }
    }

    fn rebootstrap(&mut self, path: PathBuf) {
        let engine = scanner::bootstrap(path);
        let root = Arc::clone(&engine.root);
        self.root = Some(Arc::clone(&root));
        self.index = Arc::clone(&engine.index);
        self.engine = Some(engine);
        self.focus = root.path.clone();
        self.expanded.clear();
        self.expanded.insert(root.path.clone());
        self.selected = 0;
        self.map_layout = None;
        self.map_key = None;
        self.map_cache.clear();
        self.refresh();
    }

    fn move_to_parent_row(&mut self) {
        if self.selected == 0 {
            return;
        }
        let Some(cur) = self.visible.get(self.selected).cloned() else {
            return;
        };
        let mut i = self.selected;
        while i > 0 {
            i -= 1;
            if !self.visible[i].is_parent && self.visible[i].depth() < cur.depth() {
                self.selected = i;
                return;
            }
        }
        self.selected = 0;
    }

    pub fn refresh(&mut self) {
        let mut new_visible = Vec::new();
        if let Some(root) = &self.root {
            let node = self
                .node_by_path(&self.focus)
                .unwrap_or_else(|| Arc::clone(root));
            if let Some(parent) = node.path.parent() {
                if !parent.as_os_str().is_empty() {
                    new_visible.push(VisibleRow {
                        name: "..".to_string(),
                        path: parent.to_path_buf(),
                        size: 0,
                        known: true,
                        denied: false,
                        is_dir: true,
                        is_expanded: false,
                        is_parent: true,
                        prefix: String::new(),
                        done: 0,
                        total: 0,
                    });
                }
            }
            self.collect(&node, 0, &mut new_visible, "", true, true);
        }
        self.visible = new_visible;
        if self.selected >= self.visible.len() {
            self.selected = self.visible.len().saturating_sub(1);
        }
    }

    fn collect(
        &self,
        node: &Arc<Node>,
        depth: usize,
        out: &mut Vec<VisibleRow>,
        guides: &str,
        is_last: bool,
        is_top: bool,
    ) {
        let prefix = if is_top {
            String::new()
        } else {
            let connector = if is_last { "└─ " } else { "├─ " };
            format!("{guides}{connector}")
        };
        out.push(VisibleRow {
            name: node.name.clone(),
            path: node.path.clone(),
            size: node.size(),
            known: node.is_known(),
            denied: node.is_denied(),
            is_dir: node.is_dir,
            is_expanded: self.expanded.contains(&node.path),
            is_parent: false,
            prefix,
            done: node.done_entries.load(Ordering::Relaxed),
            total: node.total_entries.load(Ordering::Relaxed),
        });

        if node.is_dir && self.expanded.contains(&node.path) {
            let mut kids = node.child_list();
            self.sort_children(&mut kids);
            let n = kids.len();
            let child_guides = if is_top {
                String::new()
            } else {
                let guide = if is_last { "   " } else { "│  " };
                format!("{guides}{guide}")
            };
            for (i, child) in kids.iter().enumerate() {
                self.collect(child, depth + 1, out, &child_guides, i + 1 == n, false);
            }
        }
    }

    fn sort_children(&self, kids: &mut [Arc<Node>]) {
        match self.sort {
            SortMode::Name => kids.sort_by(|a, b| {
                b.is_dir
                    .cmp(&a.is_dir)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
            SortMode::SizeDesc => kids.sort_by(|a, b| b.size().cmp(&a.size())),
            SortMode::SizeAsc => kids.sort_by(|a, b| a.size().cmp(&b.size())),
        }
    }

    /// Rebuild visible rows and drive the asynchronous, cached treemap build.
    pub fn pump(&mut self) {
        self.refresh();

        loop {
            match self.map_rx.try_recv() {
                Ok(res) => {
                    self.map_cache
                        .insert(res.subject.clone(), (res.sig, res.layout.clone()));
                    if res.generation == self.map_gen {
                        self.map_layout = Some(res.layout);
                        self.map_busy = false;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        let Some(subject) = self.subject_node() else {
            return;
        };
        let kids = subject.child_list();
        let hl = self.highlight_path();
        let mut sig: u64 = 1469598103934665603;
        let mut entries: Vec<MapEntry> = Vec::with_capacity(kids.len());
        for c in &kids {
            let known = c.is_known();
            let size = if known {
                c.size()
            } else {
                c.acc.load(Ordering::Relaxed)
            };
            let color = if c.is_dir {
                color::DIR_COLOR
            } else {
                color::ext_color(&c.name, false)
            };
            for byte in c.name.bytes() {
                sig = (sig ^ byte as u64).wrapping_mul(1099511628211);
            }
            sig = (sig ^ size).wrapping_mul(1099511628211);
            sig = (sig ^ known as u64).wrapping_mul(1099511628211);
            entries.push(MapEntry {
                label: c.name.clone(),
                size,
                known,
                is_dir: c.is_dir,
                color,
                highlight: hl.as_deref() == Some(c.path.as_path()),
            });
        }

        let path = subject.path.clone();

        // Fold the map's cell dimensions into the signature so a resize or a
        // moved split boundary invalidates the cached layout instead of showing
        // a stale map computed for the old geometry.
        let (rows, cols) = crate::term::terminal_size();
        let cell = self.cell_width.max(1) as usize;
        let eff_cols = (cols as usize / cell).max(1);
        let regions = self.regions(rows as usize, eff_cols);
        let (w, h) = (regions.map.w.max(1), regions.map.h.max(1));
        sig = (sig ^ w as u64).wrapping_mul(1099511628211);
        sig = (sig ^ h as u64).wrapping_mul(1099511628211);

        // Cache hit: the same subject with an unchanged signature -> show now.
        if let Some((cached_sig, cached)) = self.map_cache.get(&path) {
            if *cached_sig == sig {
                if self
                    .map_key
                    .as_ref()
                    .map(|(p, s)| p != &path || *s != sig)
                    .unwrap_or(true)
                {
                    self.map_layout = Some(cached.clone());
                    self.map_key = Some((path, sig));
                    self.map_busy = false;
                }
                return;
            }
        }

        let subject_changed = self
            .map_key
            .as_ref()
            .map(|(p, _)| p != &path)
            .unwrap_or(true);
        let sig_changed = self
            .map_key
            .as_ref()
            .map(|(_, s)| *s != sig)
            .unwrap_or(true);
        let due = self.map_last_send.elapsed() > Duration::from_millis(120);

        if subject_changed || (sig_changed && due) {
            if subject_changed {
                self.map_layout = None;
            }
            self.map_gen = self.map_gen.wrapping_add(1);
            self.map_key = Some((path.clone(), sig));

            if entries.len() <= SYNC_LIMIT {
                // Small enough to lay out synchronously — no spinner flash.
                let layout = treemap::compute(w, h, &entries);
                self.map_cache.insert(path, (sig, layout.clone()));
                self.map_layout = Some(layout);
                self.map_busy = false;
            } else {
                self.map_busy = true;
                self.map_last_send = Instant::now();
                let _ = self.map_tx.send(MapRequest {
                    generation: self.map_gen,
                    subject: path,
                    sig,
                    w,
                    h,
                    entries,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    #[test]
    fn app_bootstraps_and_lists() {
        let base = std::env::temp_dir().join("dirlook_app_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("sub")).unwrap();
        fs::write(base.join("a.txt"), vec![0u8; 100]).unwrap();
        fs::write(base.join("sub/b.txt"), vec![0u8; 200]).unwrap();

        let mut app = App::new(base.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.pump();
            if app.root.as_ref().map(|r| r.is_known()).unwrap_or(false)
                || Instant::now() > deadline
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        app.pump();
        assert!(app.visible.iter().any(|r| r.name == "sub"));
        assert!(app.map_subject().is_some());
        let _ = fs::remove_dir_all(&base);
    }
}
