use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

/// How many times a permission-denied directory is retried while the app runs.
pub const DEFAULT_DIR_RETRIES: u32 = 150;

/// A node of the live filesystem tree. Directories start with an unknown size
/// and are filled in by the background scan engine. All mutation is atomic (or
/// behind a small mutex for the child list) so workers and the UI can share it.
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// Final size in bytes, valid once `known` is true.
    pub size: AtomicU64,
    pub known: AtomicBool,
    pub children: Mutex<Vec<Arc<Node>>>,
    pub parent: Mutex<Option<Weak<Node>>>,
    /// Whether this directory has already been scheduled for scanning.
    pub queued: AtomicBool,
    /// Whether this directory's immediate children have been listed.
    pub listed: AtomicBool,
    /// Whether listing this directory failed due to missing permission.
    pub denied: AtomicBool,
    /// Remaining background retries for a permission-denied directory.
    pub retries: AtomicU32,
    /// Whether this node's size has already been added to its parent.
    pub counted: AtomicBool,
    /// Number of sub-directories still being scanned.
    pub remaining: AtomicU64,
    /// Accumulated bytes from processed children (directories).
    pub acc: AtomicU64,
    /// Immediate-entry counters, for the per-directory progress bar.
    pub total_entries: AtomicU64,
    pub done_entries: AtomicU64,
}

impl Node {
    pub fn file(name: String, path: PathBuf, size: u64) -> Arc<Node> {
        Arc::new(Node {
            name,
            path,
            is_dir: false,
            size: AtomicU64::new(size),
            known: AtomicBool::new(true),
            children: Mutex::new(Vec::new()),
            parent: Mutex::new(None),
            queued: AtomicBool::new(true),
            listed: AtomicBool::new(true),
            denied: AtomicBool::new(false),
            retries: AtomicU32::new(0),
            counted: AtomicBool::new(false),
            remaining: AtomicU64::new(0),
            acc: AtomicU64::new(size),
            total_entries: AtomicU64::new(0),
            done_entries: AtomicU64::new(1),
        })
    }

    pub fn dir(name: String, path: PathBuf, parent: Option<&Arc<Node>>) -> Arc<Node> {
        let node = Arc::new(Node {
            name,
            path,
            is_dir: true,
            size: AtomicU64::new(0),
            known: AtomicBool::new(false),
            children: Mutex::new(Vec::new()),
            parent: Mutex::new(None),
            queued: AtomicBool::new(false),
            listed: AtomicBool::new(false),
            denied: AtomicBool::new(false),
            retries: AtomicU32::new(DEFAULT_DIR_RETRIES),
            counted: AtomicBool::new(false),
            remaining: AtomicU64::new(0),
            acc: AtomicU64::new(0),
            total_entries: AtomicU64::new(0),
            done_entries: AtomicU64::new(0),
        });
        if let Some(p) = parent {
            *node.parent.lock().unwrap() = Some(Arc::downgrade(p));
        }
        node
    }

    pub fn size(&self) -> u64 {
        self.size.load(Ordering::Relaxed)
    }

    pub fn is_known(&self) -> bool {
        self.known.load(Ordering::Relaxed)
    }

    pub fn is_denied(&self) -> bool {
        self.denied.load(Ordering::Relaxed)
    }

    pub fn child_list(&self) -> Vec<Arc<Node>> {
        self.children.lock().unwrap().clone()
    }

    /// Find a node by path. Clones the child list at each level so the lock is
    /// not held while recursing (the scan engine may be mutating children).
    pub fn find(root: &Arc<Node>, path: &Path) -> Option<Arc<Node>> {
        if root.path == path {
            return Some(Arc::clone(root));
        }
        let kids = root.child_list();
        for c in &kids {
            if let Some(found) = Node::find(c, path) {
                return Some(found);
            }
        }
        None
    }
}
