use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::node::Node;

/// Live per-directory progress. `total`/`done` count the directory's *immediate*
/// children (known right after listing, no extra pass); `partial` is the size of
/// the subtree found so far.
pub struct DirNode {
    pub total: u64,
    pub done: AtomicU64,
    pub partial: AtomicU64,
}

/// One immediate child of the scanned root, shown in the progress list.
pub struct RootChild {
    pub name: String,
}

pub struct ScanStatus {
    pub chain: Mutex<Vec<Arc<DirNode>>>,
    pub root_children: Mutex<Vec<Arc<RootChild>>>,
    pub dirs: AtomicU64,
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub done: AtomicBool,
    pub cancelled: AtomicBool,
}

impl ScanStatus {
    fn new() -> Self {
        Self {
            chain: Mutex::new(Vec::new()),
            root_children: Mutex::new(Vec::new()),
            dirs: AtomicU64::new(0),
            files: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            done: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        }
    }
}

pub struct Scan {
    pub status: Arc<ScanStatus>,
    pub rx: Receiver<Node>,
}

/// Start a scan on a detached background thread.
pub fn scan_async(root: PathBuf) -> Scan {
    let status = Arc::new(ScanStatus::new());
    let (tx, rx) = channel();
    let st = Arc::clone(&status);
    let _ = thread::spawn(move || {
        let node = scan_root(&root, &st);
        st.done.store(true, Ordering::Relaxed);
        let _ = tx.send(node);
    });
    Scan { status, rx }
}

/// Synchronous scan (kept for tests and simple use).
#[allow(dead_code)]
pub fn scan(root: &Path) -> Node {
    let status = Arc::new(ScanStatus::new());
    scan_root(root, &status)
}

fn scan_root(root: &Path, st: &Arc<ScanStatus>) -> Node {
    let canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let name = canon
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| canon.to_string_lossy().to_string());

    let mut ancestors: Vec<Arc<DirNode>> = Vec::new();
    let (size, children) = scan_dir(&canon, st, &mut ancestors, true);
    let mut node = Node::dir(name, canon, size, children);
    node.expanded = true;
    node
}

fn scan_dir(
    dir: &Path,
    st: &Arc<ScanStatus>,
    ancestors: &mut Vec<Arc<DirNode>>,
    is_root: bool,
) -> (u64, Vec<Node>) {
    if st.cancelled.load(Ordering::Relaxed) {
        return (0, Vec::new());
    }

    // List immediate children first so `total` is known up front.
    let entries: Vec<fs::DirEntry> = fs::read_dir(dir)
        .map(|it| it.filter_map(Result::ok).collect())
        .unwrap_or_default();

    // For the scanned root, publish the immediate children for the progress list.
    if is_root {
        let rc: Vec<Arc<RootChild>> = entries
            .iter()
            .map(|e| {
                Arc::new(RootChild {
                    name: e.file_name().to_string_lossy().to_string(),
                })
            })
            .collect();
        if let Ok(mut list) = st.root_children.lock() {
            *list = rc;
        }
    }

    let me = Arc::new(DirNode {
        total: entries.len() as u64,
        done: AtomicU64::new(0),
        partial: AtomicU64::new(0),
    });
    ancestors.push(Arc::clone(&me));
    st.dirs.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut chain) = st.chain.lock() {
        chain.push(Arc::clone(&me));
    }

    let mut total: u64 = 0;
    let mut dirs: Vec<Node> = Vec::new();
    let mut files: Vec<(String, PathBuf, u64)> = Vec::new();

    for entry in entries {
        if st.cancelled.load(Ordering::Relaxed) {
            break;
        }
        let path = entry.path();
        match entry.metadata() {
            Ok(meta) if meta.is_dir() => {
                let dn = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let (dir_size, children) = scan_dir(&path, st, ancestors, false);
                total += dir_size;
                dirs.push(Node::dir(dn, path, dir_size, children));
            }
            Ok(meta) if meta.is_file() => {
                let size = meta.len();
                total += size;
                st.files.fetch_add(1, Ordering::Relaxed);
                st.bytes.fetch_add(size, Ordering::Relaxed);
                for a in ancestors.iter() {
                    a.partial.fetch_add(size, Ordering::Relaxed);
                }
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                files.push((file_name, path, size));
            }
            _ => {}
        }
        me.done.fetch_add(1, Ordering::Relaxed);
    }

    ancestors.pop();
    if let Ok(mut chain) = st.chain.lock() {
        chain.pop();
    }

    dirs.sort_by(|a, b| b.size.cmp(&a.size));
    files.sort_by(|a, b| b.2.cmp(&a.2));

    let mut nodes = dirs;
    for (name, path, size) in files.into_iter().take(2000) {
        nodes.push(Node::file(name, path, size));
    }

    (total, nodes)
}
