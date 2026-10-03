use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    pub children: Vec<Node>,
    pub expanded: bool,
}

impl Node {
    pub fn file(name: String, path: PathBuf, size: u64) -> Self {
        Self {
            name,
            path,
            size,
            is_dir: false,
            children: Vec::new(),
            expanded: false,
        }
    }

    pub fn dir(name: String, path: PathBuf, size: u64, children: Vec<Node>) -> Self {
        Self {
            name,
            path,
            size,
            is_dir: true,
            children,
            expanded: false,
        }
    }

    pub fn find<'a>(node: &'a Node, path: &Path) -> Option<&'a Node> {
        if node.path == path {
            return Some(node);
        }
        for child in &node.children {
            if let Some(found) = Node::find(child, path) {
                return Some(found);
            }
        }
        None
    }

    pub fn find_mut<'a>(node: &'a mut Node, path: &Path) -> Option<&'a mut Node> {
        if node.path == path {
            return Some(node);
        }
        for child in &mut node.children {
            if let Some(found) = Node::find_mut(child, path) {
                return Some(found);
            }
        }
        None
    }

    pub fn sort_children(node: &mut Node, desc: bool) {
        node.children
            .sort_by(|a, b| if desc { b.size.cmp(&a.size) } else { a.size.cmp(&b.size) });
        for child in &mut node.children {
            Node::sort_children(child, desc);
        }
    }
}
