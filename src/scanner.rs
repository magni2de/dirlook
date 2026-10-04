use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::node::Node;

pub type Index = Arc<Mutex<HashMap<PathBuf, Arc<Node>>>>;

struct Queue {
    high: Mutex<VecDeque<Arc<Node>>>,
    low: Mutex<VecDeque<Arc<Node>>>,
    cancel: AtomicBool,
}

impl Queue {
    fn push(&self, node: Arc<Node>, high: bool) {
        if node.queued.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut q = if high {
            self.high.lock().unwrap()
        } else {
            self.low.lock().unwrap()
        };
        q.push_back(node);
    }

    fn pop(&self) -> Option<Arc<Node>> {
        if let Some(n) = self.high.lock().unwrap().pop_front() {
            return Some(n);
        }
        self.low.lock().unwrap().pop_front()
    }
}

/// Background, multi-threaded scan engine for one scanned root.
pub struct Engine {
    pub root: Arc<Node>,
    pub index: Index,
    queue: Arc<Queue>,
    handles: Vec<thread::JoinHandle<()>>,
}

impl Engine {
    /// Ask a directory's (not yet scheduled) subtree to be scanned first.
    pub fn boost(&self, path: &Path, high: bool) {
        let node = self.index.lock().unwrap().get(path).cloned();
        if let Some(n) = node {
            self.queue.push(n, high);
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.queue.cancel.store(true, Ordering::Relaxed);
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

pub fn bootstrap(root_path: PathBuf) -> Engine {
    let canon = fs::canonicalize(&root_path).unwrap_or(root_path);
    let name = canon
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| canon.to_string_lossy().to_string());
    let root = Node::dir(name, canon.clone(), None);
    let index: Index = Arc::new(Mutex::new(HashMap::new()));
    index.lock().unwrap().insert(canon, Arc::clone(&root));

    let queue = Arc::new(Queue {
        high: Mutex::new(VecDeque::new()),
        low: Mutex::new(VecDeque::new()),
        cancel: AtomicBool::new(false),
    });
    queue.push(Arc::clone(&root), true);

    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let q = Arc::clone(&queue);
        let idx = Arc::clone(&index);
        handles.push(thread::spawn(move || worker(q, idx)));
    }

    Engine {
        root,
        index,
        queue,
        handles,
    }
}

fn worker(queue: Arc<Queue>, index: Index) {
    loop {
        if queue.cancel.load(Ordering::Relaxed) {
            return;
        }
        match queue.pop() {
            Some(node) => process_dir(&node, &queue, &index),
            None => thread::sleep(Duration::from_millis(2)),
        }
    }
}

fn process_dir(node: &Arc<Node>, queue: &Arc<Queue>, index: &Index) {
    let entries: Vec<fs::DirEntry> = match fs::read_dir(&node.path) {
        Ok(it) => it.filter_map(Result::ok).collect(),
        Err(_) => {
            node.listed.store(true, Ordering::Relaxed);
            finalize(node);
            return;
        }
    };

    node.total_entries
        .store(entries.len() as u64, Ordering::Relaxed);

    let mut dir_children: Vec<Arc<Node>> = Vec::new();
    for entry in entries {
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                node.done_entries.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if meta.is_dir() {
            let child = Node::dir(name, path.clone(), Some(node));
            node.children.lock().unwrap().push(Arc::clone(&child));
            index.lock().unwrap().insert(path, Arc::clone(&child));
            dir_children.push(child);
        } else if meta.is_file() {
            let size = meta.len();
            let child = Node::file(name, path.clone(), size);
            node.children.lock().unwrap().push(Arc::clone(&child));
            index.lock().unwrap().insert(path, Arc::clone(&child));
            node.acc.fetch_add(size, Ordering::Relaxed);
            node.done_entries.fetch_add(1, Ordering::Relaxed);
        } else {
            node.done_entries.fetch_add(1, Ordering::Relaxed);
        }
    }

    node.listed.store(true, Ordering::Relaxed);
    node.remaining
        .store(dir_children.len() as u64, Ordering::Relaxed);

    if dir_children.is_empty() {
        finalize(node);
    } else {
        for c in dir_children {
            queue.push(c, false);
        }
    }
}

/// Mark a directory known and propagate its size up to its ancestors.
fn finalize(node: &Arc<Node>) {
    let mut cur = Arc::clone(node);
    loop {
        cur.size.store(cur.acc.load(Ordering::Relaxed), Ordering::Relaxed);
        cur.known.store(true, Ordering::Relaxed);

        let parent = cur
            .parent
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|w| w.upgrade());
        let Some(p) = parent else {
            break;
        };
        p.acc.fetch_add(cur.size.load(Ordering::Relaxed), Ordering::Relaxed);
        p.done_entries.fetch_add(1, Ordering::Relaxed);
        if p.remaining.fetch_sub(1, Ordering::Relaxed) == 1 {
            cur = p;
        } else {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn engine_scans_small_tree() {
        let base = std::env::temp_dir().join("dirlook_engine_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("sub")).unwrap();
        fs::write(base.join("a.bin"), vec![0u8; 1000]).unwrap();
        fs::write(base.join("sub/b.bin"), vec![0u8; 2000]).unwrap();
        fs::write(base.join("sub/c.bin"), vec![0u8; 3000]).unwrap();

        let engine = bootstrap(base.clone());
        let root = Arc::clone(&engine.root);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !root.is_known() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(root.is_known(), "root not finalized in time");
        assert_eq!(root.size(), 6000);

        drop(engine);
        let _ = fs::remove_dir_all(&base);
    }
}
