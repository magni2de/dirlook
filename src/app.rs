use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Instant;

use crate::key::KeyCode;
use crate::node::Node;
use crate::scanner::{self, ScanStatus};

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum ViewMode {
    Tree,
    Flat,
    Treemap,
}

#[derive(Debug, Clone)]
pub struct VisibleRow {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub depth: usize,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub is_parent: bool,
}

pub struct ScanState {
    pub target: PathBuf,
    pub status: Arc<ScanStatus>,
    pub rx: Receiver<Node>,
    pub started: Instant,
    pub tick: u64,
}

pub struct App {
    pub root: Option<Node>,
    pub visible: Vec<VisibleRow>,
    pub selected: usize,
    #[allow(dead_code)]
    pub view_mode: ViewMode,
    pub sort_desc: bool,
    pub quit: bool,
    pub cell_width: u32,
    pub scanning: Option<ScanState>,
    pub show_legend: bool,
    pub focus: PathBuf,
    pending_select: Option<PathBuf>,
}

impl App {
    pub fn empty() -> Self {
        Self {
            root: None,
            visible: Vec::new(),
            selected: 0,
            view_mode: ViewMode::Tree,
            sort_desc: true,
            quit: false,
            cell_width: 1,
            scanning: None,
            show_legend: false,
            focus: PathBuf::new(),
            pending_select: None,
        }
    }

    #[allow(dead_code)]
    pub fn new(root: Node) -> Self {
        let mut app = Self::empty();
        app.focus = root.path.clone();
        app.root = Some(root);
        app.rebuild_visible();
        app.selected = app
            .visible
            .iter()
            .position(|r| !r.is_parent)
            .unwrap_or(0);
        app
    }

    /// Kick off a background scan, to be picked up by [`App::pump_scan`].
    pub fn start_scan(&mut self, path: PathBuf) {
        let scan = scanner::scan_async(path.clone());
        self.scanning = Some(ScanState {
            target: path,
            status: scan.status,
            rx: scan.rx,
            started: Instant::now(),
            tick: 0,
        });
    }

    pub fn tick(&mut self) {
        if let Some(s) = &mut self.scanning {
            s.tick = s.tick.wrapping_add(1);
        }
    }

    pub fn cancel_scan(&mut self) {
        if let Some(s) = self.scanning.take() {
            s.status.cancelled.store(true, Ordering::Relaxed);
        }
    }

    /// Returns true if a finished scan was installed.
    pub fn pump_scan(&mut self) -> bool {
        if self.scanning.is_none() {
            return false;
        }
        let result = self.scanning.as_ref().unwrap().rx.try_recv();
        match result {
            Ok(node) => {
                self.focus = node.path.clone();
                self.root = Some(node);
                self.rebuild_visible();
                let want = self.pending_select.take();
                self.selected = want
                    .and_then(|p| {
                        self.visible
                            .iter()
                            .position(|r| !r.is_parent && r.path == p)
                    })
                    .or_else(|| self.visible.iter().position(|r| !r.is_parent))
                    .unwrap_or(0);
                self.scanning = None;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.scanning = None;
                true
            }
        }
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        if self.scanning.is_some() {
            match code {
                KeyCode::Char('q') => {
                    self.cancel_scan();
                    self.quit = true;
                }
                KeyCode::Escape => {
                    self.cancel_scan();
                    if self.root.is_none() {
                        self.quit = true;
                    }
                }
                _ => {}
            }
            return;
        }

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
                            self.toggle_expand(&row.path, true);
                        } else if let Some(neighbor) = self.visible.get(self.selected + 1) {
                            if neighbor.depth == row.depth + 1 && !neighbor.is_parent {
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
                        // nothing to do
                    } else if row.is_dir && row.is_expanded {
                        self.toggle_expand(&row.path, false);
                    } else if row.depth > 0 {
                        self.move_to_parent(row.depth);
                    }
                }
            }
            KeyCode::Char('s') => {
                self.sort_desc = !self.sort_desc;
                if let Some(root) = &mut self.root {
                    Node::sort_children(root, self.sort_desc);
                }
                self.rebuild_visible();
            }
            _ => {}
        }
    }

    fn ascend(&mut self) {
        let cur = match &self.root {
            Some(r) => r.path.clone(),
            None => return,
        };
        let parent = match cur.parent() {
            Some(p) => p.to_path_buf(),
            None => return,
        };
        self.pending_select = Some(cur);
        self.start_scan(parent);
    }

    /// Enter a directory: show it as the top of the tree (instant, no rescan).
    fn enter_dir(&mut self, path: &std::path::Path) {
        self.focus = path.to_path_buf();
        if let Some(root) = &mut self.root {
            if let Some(node) = Node::find_mut(root, path) {
                node.expanded = true;
            }
        }
        self.rebuild_visible();
        self.selected = self
            .visible
            .iter()
            .position(|r| !r.is_parent)
            .unwrap_or(0);
    }

    /// Go up one level: within the loaded tree if possible, else rescan the
    /// parent of the scanned root.
    fn up_or_ascend(&mut self) {
        let root_path = match &self.root {
            Some(r) => r.path.clone(),
            None => return,
        };
        if self.focus == root_path {
            self.ascend();
            return;
        }
        let from = self.focus.clone();
        if let Some(parent) = self.focus.parent() {
            self.focus = parent.to_path_buf();
            self.rebuild_visible();
            self.selected = self
                .visible
                .iter()
                .position(|r| !r.is_parent && r.path == from)
                .or_else(|| self.visible.iter().position(|r| !r.is_parent))
                .unwrap_or(0);
        }
    }

    fn toggle_expand(&mut self, path: &std::path::Path, expanded: bool) {
        if let Some(root) = &mut self.root {
            if let Some(node) = Node::find_mut(root, path) {
                node.expanded = expanded;
            }
        }
        self.rebuild_visible();
    }

    fn move_to_parent(&mut self, current_depth: usize) {
        if self.selected == 0 {
            return;
        }
        let mut i = self.selected;
        while i > 0 {
            i -= 1;
            if !self.visible[i].is_parent && self.visible[i].depth < current_depth {
                self.selected = i;
                return;
            }
        }
        self.selected = 0;
    }

    fn rebuild_visible(&mut self) {
        let mut new_visible = Vec::new();
        if let Some(root) = &self.root {
            let node = Node::find(root, &self.focus).unwrap_or(root);
            if let Some(parent) = node.path.parent() {
                if !parent.as_os_str().is_empty() {
                    new_visible.push(VisibleRow {
                        name: "..".to_string(),
                        path: parent.to_path_buf(),
                        size: 0,
                        depth: 0,
                        is_dir: true,
                        is_expanded: false,
                        is_parent: true,
                    });
                }
            }
            Self::collect_into(node, 0, &mut new_visible);
        }
        self.visible = new_visible;
        if self.selected >= self.visible.len() {
            self.selected = self.visible.len().saturating_sub(1);
        }
    }

    fn collect_into(node: &Node, depth: usize, out: &mut Vec<VisibleRow>) {
        out.push(VisibleRow {
            name: node.name.clone(),
            path: node.path.clone(),
            size: node.size,
            depth,
            is_dir: node.is_dir,
            is_expanded: node.expanded,
            is_parent: false,
        });
        if node.expanded {
            for child in &node.children {
                Self::collect_into(child, depth + 1, out);
            }
        }
    }

    /// Node whose children are shown in the treemap.
    pub fn map_subject(&self) -> Option<&Node> {
        let root = self.root.as_ref()?;
        let row = self.visible.get(self.selected)?;
        if row.is_parent {
            return Some(root);
        }
        let node = Node::find(root, &row.path)?;
        if node.is_dir {
            Some(node)
        } else {
            let parent_path = node.path.parent()?;
            Node::find(root, parent_path)
        }
    }

    /// Path of the row to highlight in the treemap (None for the `..` row).
    pub fn highlight_path(&self) -> Option<PathBuf> {
        let row = self.visible.get(self.selected)?;
        if row.is_parent {
            None
        } else {
            Some(row.path.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner;

    #[test]
    fn root_expanded_and_parent_row() {
        let node = scanner::scan(std::path::Path::new("src"));
        assert!(node.expanded);
        let app = App::new(node);
        assert!(app.visible.first().map(|r| r.is_parent).unwrap_or(false));
        assert!(app.visible.iter().filter(|r| !r.is_parent).count() > 1);
        assert!(!app.visible[app.selected].is_parent);
        assert!(app.map_subject().is_some());
    }

    #[test]
    fn others_includes_small_dirs() {
        use std::fs;
        let base = std::env::temp_dir().join("dirlook_others_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("smalldir")).unwrap();
        fs::create_dir_all(base.join("dir2")).unwrap();
        fs::write(base.join("smalldir/a.txt"), vec![0u8; 10_000]).unwrap();
        fs::write(base.join("dir2/b.txt"), vec![0u8; 3_000]).unwrap();
        fs::write(base.join("small.txt"), vec![0u8; 5_000]).unwrap();
        fs::write(base.join("big.bin"), vec![0u8; 5_000_000]).unwrap();

        let node = scanner::scan(&base);
        let mut ch: Vec<&Node> = node.children.iter().collect();
        ch.sort_by(|a, b| b.size.cmp(&a.size));
        let entries: Vec<(String, u64, (u8, u8, u8))> = ch
            .iter()
            .map(|c| (c.name.clone(), c.size, (0, 0, 0)))
            .collect();
        let data = crate::treemap::build_data(10, &entries, None);
        let others = data.others.expect("others");
        assert_eq!(
            others.size, 18_000,
            "count={} size={} (smalldir 10000 + small.txt 5000 + dir2 3000)",
            others.count, others.size
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn enter_dir_focuses_child() {
        use std::fs;
        let base = std::env::temp_dir().join("dirlook_focus_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("sub")).unwrap();
        fs::write(base.join("sub/a.txt"), vec![0u8; 100]).unwrap();
        fs::write(base.join("top.txt"), vec![0u8; 50]).unwrap();

        let node = scanner::scan(&base);
        let mut app = App::new(node);
        let sub_path = app
            .visible
            .iter()
            .find(|r| r.is_dir && !r.is_parent)
            .map(|r| r.path.clone())
            .expect("a subdirectory row");

        app.enter_dir(&sub_path);
        assert_eq!(app.focus, sub_path);
        let first = app
            .visible
            .iter()
            .find(|r| !r.is_parent)
            .expect("focus row");
        assert_eq!(first.path, sub_path);
        let _ = fs::remove_dir_all(&base);
    }
}
