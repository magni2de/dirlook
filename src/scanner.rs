use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::node::Node;

pub type Index = Arc<Mutex<HashMap<PathBuf, Arc<Node>>>>;

/// Delay between background retries of a permission-denied directory.
const RETRY_DELAY: Duration = Duration::from_secs(2);

struct Queue {
    high: Mutex<VecDeque<Arc<Node>>>,
    low: Mutex<VecDeque<Arc<Node>>>,
    retries: Mutex<Vec<(Instant, Arc<Node>)>>,
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

    fn schedule_retry(&self, node: Arc<Node>, at: Instant) {
        self.retries.lock().unwrap().push((at, node));
    }

    fn repush_due(&self) {
        let now = Instant::now();
        let mut due = Vec::new();
        {
            let mut r = self.retries.lock().unwrap();
            let mut kept = Vec::with_capacity(r.len());
            for (at, n) in r.drain(..) {
                if at <= now {
                    due.push(n);
                } else {
                    kept.push((at, n));
                }
            }
            *r = kept;
        }
        for n in due {
            self.push(n, false);
        }
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

/// Replace the children of `live` with `shadow`'s subtree, fixing the index and
/// the ancestor sizes. Returns true if anything actually changed.
///
/// `live` keeps its identity (and stays `known`), so the view never flickers
/// into a "scanning" state — this is the atomic swap behind auto-refresh.
pub fn splice(index: &Index, live: &Arc<Node>, shadow: &Arc<Node>) -> bool {
    if !children_changed(live, shadow) {
        return false;
    }

    let old_size = live.size();
    let new_size = shadow.size();

    {
        let mut idx = index.lock().unwrap();
        remove_index_subtree(live, &mut idx);
    }
    detach_subtree(live);
    live.children.lock().unwrap().clear();

    let new_children = shadow.child_list();
    {
        let mut idx = index.lock().unwrap();
        for c in &new_children {
            adopt_subtree(&mut idx, c, live);
        }
    }
    *live.children.lock().unwrap() = new_children;

    // Fold the size difference into every ancestor.
    let delta = new_size as i64 - old_size as i64;
    if delta != 0 {
        let mut parent = live.parent.lock().unwrap().as_ref().and_then(|w| w.upgrade());
        while let Some(p) = parent {
            let acc = (p.acc.load(Ordering::Relaxed) as i64 + delta).max(0) as u64;
            p.acc.store(acc, Ordering::Relaxed);
            let size = (p.size.load(Ordering::Relaxed) as i64 + delta).max(0) as u64;
            p.size.store(size, Ordering::Relaxed);
            parent = p.parent.lock().unwrap().as_ref().and_then(|w| w.upgrade());
        }
    }

    let total = shadow.total_entries.load(Ordering::Relaxed);
    live.acc.store(new_size, Ordering::Relaxed);
    live.size.store(new_size, Ordering::Relaxed);
    live.known.store(true, Ordering::Relaxed);
    live.listed.store(true, Ordering::Relaxed);
    live.denied.store(shadow.denied.load(Ordering::Relaxed), Ordering::Relaxed);
    live.total_entries.store(total, Ordering::Relaxed);
    live.done_entries.store(total, Ordering::Relaxed);
    live.remaining.store(0, Ordering::Relaxed);
    true
}

/// Whether `shadow`'s immediate children differ from `live`'s (order-insensitive).
fn children_changed(live: &Arc<Node>, shadow: &Arc<Node>) -> bool {
    if live.size() != shadow.size() {
        return true;
    }
    let key = |c: &Arc<Node>| {
        (
            c.name.clone(),
            c.is_dir,
            c.size(),
            c.known.load(Ordering::Relaxed),
        )
    };
    let mut a: Vec<_> = live.child_list().iter().map(key).collect();
    let mut b: Vec<_> = shadow.child_list().iter().map(key).collect();
    a.sort();
    b.sort();
    a != b
}

/// Remove every descendant of `root` (not `root` itself) from the index.
fn remove_index_subtree(root: &Arc<Node>, idx: &mut HashMap<PathBuf, Arc<Node>>) {
    let mut stack = root.child_list();
    while let Some(n) = stack.pop() {
        idx.remove(&n.path);
        stack.extend(n.child_list());
    }
}

/// Detach every descendant of `root` from its parent, so a worker that is still
/// finalizing a node from the discarded subtree cannot add a stale size back.
fn detach_subtree(root: &Arc<Node>) {
    let mut stack = root.child_list();
    while let Some(n) = stack.pop() {
        *n.parent.lock().unwrap() = None;
        n.counted.store(true, Ordering::Relaxed);
        stack.extend(n.child_list());
    }
}

/// Re-parent a shadow subtree under `parent` and index every node in it.
fn adopt_subtree(idx: &mut HashMap<PathBuf, Arc<Node>>, node: &Arc<Node>, parent: &Arc<Node>) {
    let mut stack = vec![(Arc::clone(node), Arc::clone(parent))];
    while let Some((n, p)) = stack.pop() {
        *n.parent.lock().unwrap() = Some(Arc::downgrade(&p));
        idx.insert(n.path.clone(), Arc::clone(&n));
        for c in n.child_list() {
            stack.push((c, Arc::clone(&n)));
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
    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    bootstrap_with(root_path, workers)
}

/// Like [`bootstrap`], but with an explicit worker count — used for the light
/// background refresh scans so they do not spawn a full pool every time.
pub fn bootstrap_n(root_path: PathBuf, workers: usize) -> Engine {
    bootstrap_with(root_path, workers.clamp(1, 8))
}

fn bootstrap_with(root_path: PathBuf, workers: usize) -> Engine {
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
        retries: Mutex::new(Vec::new()),
        cancel: AtomicBool::new(false),
    });
    queue.push(Arc::clone(&root), true);

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
        queue.repush_due();
        match queue.pop() {
            Some(node) => process_dir(&node, &queue, &index),
            None => thread::sleep(Duration::from_millis(2)),
        }
    }
}

fn process_dir(node: &Arc<Node>, queue: &Arc<Queue>, index: &Index) {
    let entries: Vec<fs::DirEntry> = match fs::read_dir(&node.path) {
        Ok(it) => {
            node.denied.store(false, Ordering::Relaxed);
            it.filter_map(Result::ok).collect()
        }
        Err(e) => {
            node.listed.store(true, Ordering::Relaxed);
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                node.denied.store(true, Ordering::Relaxed);
                finalize(node);
                if node.retries.load(Ordering::Relaxed) > 0 {
                    node.retries.fetch_sub(1, Ordering::Relaxed);
                    node.queued.store(false, Ordering::Relaxed);
                    queue.schedule_retry(Arc::clone(node), Instant::now() + RETRY_DELAY);
                }
            } else {
                finalize(node);
            }
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
        let size = cur.acc.load(Ordering::Relaxed);
        cur.size.store(size, Ordering::Relaxed);
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

        if cur.counted.swap(true, Ordering::SeqCst) {
            propagate(p, size);
            break;
        }

        p.acc.fetch_add(size, Ordering::Relaxed);
        p.done_entries.fetch_add(1, Ordering::Relaxed);
        if p.remaining.fetch_sub(1, Ordering::Relaxed) == 1 {
            cur = p;
        } else {
            break;
        }
    }
}

/// Add a size delta to a node and all of its ancestors. Used when a previously
/// finalized (permission-denied) directory is finally scanned successfully.
fn propagate(mut node: Arc<Node>, delta: u64) {
    loop {
        node.acc.fetch_add(delta, Ordering::Relaxed);
        node.size.fetch_add(delta, Ordering::Relaxed);
        let parent = node
            .parent
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|w| w.upgrade());
        match parent {
            Some(p) => node = p,
            None => break,
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

    #[cfg(unix)]
    #[test]
    fn engine_marks_denied_directory() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join("dirlook_denied_test");
        let _ = fs::set_permissions(
            base.join("locked"),
            fs::Permissions::from_mode(0o700),
        );
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("locked")).unwrap();
        fs::write(base.join("locked/secret.bin"), vec![0u8; 100]).unwrap();
        let locked_path = base.join("locked");
        fs::set_permissions(&locked_path, fs::Permissions::from_mode(0o000)).unwrap();

        if fs::read_dir(&locked_path).is_ok() {
            let _ = fs::set_permissions(&locked_path, fs::Permissions::from_mode(0o700));
            let _ = fs::remove_dir_all(&base);
            return;
        }

        let engine = bootstrap(base.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut locked = None;
        while Instant::now() < deadline {
            for child in engine.root.child_list() {
                if child.name == "locked" && child.is_known() {
                    locked = Some(child);
                    break;
                }
            }
            if locked.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        drop(engine);
        let _ = fs::set_permissions(&locked_path, fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&base);

        let locked = locked.expect("locked directory not finalized in time");
        assert!(locked.is_denied(), "locked directory should be marked denied");
    }

    #[test]
    fn splice_swaps_in_rescanned_subtree() {
        fn wait_known(node: &Arc<Node>) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !node.is_known() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(node.is_known(), "node not finalized in time");
        }

        let base = std::env::temp_dir().join("dirlook_splice_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("sub")).unwrap();
        fs::write(base.join("a.bin"), vec![0u8; 1000]).unwrap();

        let live = bootstrap(base.clone());
        wait_known(&live.root);

        // A shadow scan of an unchanged tree must be a no-op.
        let shadow = bootstrap_n(base.clone(), 1);
        wait_known(&shadow.root);
        assert!(!splice(&live.index, &live.root, &shadow.root));
        drop(shadow);

        // Add a file; the rescanned shadow must be swapped in.
        fs::write(base.join("b.bin"), vec![0u8; 500]).unwrap();
        let shadow = bootstrap_n(base.clone(), 1);
        wait_known(&shadow.root);
        assert!(splice(&live.index, &live.root, &shadow.root));

        assert_eq!(live.root.size(), 1500);
        let b = live.root.path.join("b.bin");
        assert!(live.root.child_list().iter().any(|c| c.path == b));
        assert!(live.index.lock().unwrap().contains_key(&b));

        drop(live);
        drop(shadow);
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn engine_retries_denied_directory() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join("dirlook_retry_test");
        let _ = fs::set_permissions(
            base.join("locked"),
            fs::Permissions::from_mode(0o700),
        );
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("locked")).unwrap();
        fs::write(base.join("locked/secret.bin"), vec![0u8; 100]).unwrap();
        let locked_path = base.join("locked");
        fs::set_permissions(&locked_path, fs::Permissions::from_mode(0o000)).unwrap();

        if fs::read_dir(&locked_path).is_ok() {
            let _ = fs::set_permissions(&locked_path, fs::Permissions::from_mode(0o700));
            let _ = fs::remove_dir_all(&base);
            return;
        }

        let engine = bootstrap(base.clone());
        let root = Arc::clone(&engine.root);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut locked = None;
        while Instant::now() < deadline {
            for child in root.child_list() {
                if child.name == "locked" && child.is_denied() {
                    locked = Some(child);
                    break;
                }
            }
            if locked.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let locked = locked.expect("locked directory not marked denied in time");
        assert_eq!(locked.size(), 0);

        fs::set_permissions(&locked_path, fs::Permissions::from_mode(0o700)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        while (!locked.is_known() || locked.size() != 100) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!locked.is_denied(), "denied flag should clear after retry");
        assert_eq!(locked.size(), 100, "retry should pick up the real size");
        assert_eq!(root.size(), 100, "retry size should propagate to ancestors");

        drop(engine);
        let _ = fs::set_permissions(&locked_path, fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&base);
    }
}
