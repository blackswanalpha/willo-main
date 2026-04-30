//! In-memory writable filesystem.
//!
//! TmpFs lives entirely on the heap and is reconstructed from scratch on every
//! boot — `fs::mount_root` calls `TmpFs::new()` once per kernel start, so the
//! contents are inherently lost on reboot. There is no persistence hook.
//!
//! Storage model is intentionally similar to `RamFs` but uses owned `Vec<u8>`
//! file payloads (so writes work) and an interior-mutable root behind a
//! `spin::Mutex`, which is what the `&self` `FileSystem` trait surface needs.

use super::{FileSystem, FsError};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

enum Node {
    File(Vec<u8>),
    Dir(BTreeMap<String, Node>),
}

pub struct TmpFs {
    root: Mutex<Node>,
}

impl Default for TmpFs {
    fn default() -> Self {
        Self::new()
    }
}

impl TmpFs {
    pub fn new() -> Self {
        Self {
            root: Mutex::new(Node::Dir(BTreeMap::new())),
        }
    }
}

fn split_segments(rel: &str) -> Vec<&str> {
    rel.split('/').filter(|s| !s.is_empty()).collect()
}

fn split_parent(rel: &str) -> Option<(Vec<&str>, &str)> {
    let segs = split_segments(rel);
    let (last, rest) = segs.split_last()?;
    Some((rest.to_vec(), last))
}

fn walk<'a>(node: &'a Node, segs: &[&str]) -> Result<&'a Node, FsError> {
    let mut cur = node;
    for s in segs {
        let dir = match cur {
            Node::Dir(d) => d,
            _ => return Err(FsError::NotADir),
        };
        cur = dir.get(*s).ok_or(FsError::NotFound)?;
    }
    Ok(cur)
}

fn walk_mut<'a>(node: &'a mut Node, segs: &[&str]) -> Result<&'a mut Node, FsError> {
    let mut cur = node;
    for s in segs {
        let dir = match cur {
            Node::Dir(d) => d,
            _ => return Err(FsError::NotADir),
        };
        cur = dir.get_mut(*s).ok_or(FsError::NotFound)?;
    }
    Ok(cur)
}

impl FileSystem for TmpFs {
    fn read_file(&self, rel: &str) -> Result<Vec<u8>, FsError> {
        let segs = split_segments(rel);
        let root = self.root.lock();
        match walk(&root, &segs)? {
            Node::File(b) => Ok(b.clone()),
            Node::Dir(_) => Err(FsError::IsADir),
        }
    }

    fn list_dir(&self, rel: &str) -> Result<Vec<String>, FsError> {
        let segs = split_segments(rel);
        let root = self.root.lock();
        match walk(&root, &segs)? {
            Node::Dir(d) => Ok(d.keys().cloned().collect()),
            _ => Err(FsError::NotADir),
        }
    }

    fn is_dir(&self, rel: &str) -> bool {
        let segs = split_segments(rel);
        let root = self.root.lock();
        matches!(walk(&root, &segs), Ok(Node::Dir(_)))
    }

    fn writable(&self) -> bool {
        true
    }

    fn backing(&self) -> &'static str {
        "tmpfs"
    }

    fn create_file(&self, rel: &str) -> Result<(), FsError> {
        let (parent, name) = split_parent(rel).ok_or(FsError::Io)?;
        let mut root = self.root.lock();
        let parent_node = walk_mut(&mut root, &parent)?;
        let dir = match parent_node {
            Node::Dir(d) => d,
            _ => return Err(FsError::NotADir),
        };
        // Idempotent like FAT — re-touch truncates.
        match dir.get(name) {
            Some(Node::Dir(_)) => return Err(FsError::IsADir),
            _ => {
                dir.insert(name.to_string(), Node::File(Vec::new()));
            }
        }
        Ok(())
    }

    fn write_file(&self, rel: &str, data: &[u8]) -> Result<(), FsError> {
        let (parent, name) = split_parent(rel).ok_or(FsError::Io)?;
        let mut root = self.root.lock();
        let parent_node = walk_mut(&mut root, &parent)?;
        let dir = match parent_node {
            Node::Dir(d) => d,
            _ => return Err(FsError::NotADir),
        };
        if let Some(Node::Dir(_)) = dir.get(name) {
            return Err(FsError::IsADir);
        }
        dir.insert(name.to_string(), Node::File(data.to_vec()));
        Ok(())
    }

    fn append_file(&self, rel: &str, data: &[u8]) -> Result<(), FsError> {
        let (parent, name) = split_parent(rel).ok_or(FsError::Io)?;
        let mut root = self.root.lock();
        let parent_node = walk_mut(&mut root, &parent)?;
        let dir = match parent_node {
            Node::Dir(d) => d,
            _ => return Err(FsError::NotADir),
        };
        match dir.get_mut(name) {
            Some(Node::File(b)) => {
                b.extend_from_slice(data);
                Ok(())
            }
            Some(Node::Dir(_)) => Err(FsError::IsADir),
            None => {
                dir.insert(name.to_string(), Node::File(data.to_vec()));
                Ok(())
            }
        }
    }

    fn create_dir(&self, rel: &str) -> Result<(), FsError> {
        let (parent, name) = split_parent(rel).ok_or(FsError::Io)?;
        let mut root = self.root.lock();
        let parent_node = walk_mut(&mut root, &parent)?;
        let dir = match parent_node {
            Node::Dir(d) => d,
            _ => return Err(FsError::NotADir),
        };
        if dir.contains_key(name) {
            return Err(FsError::Io); // EEXIST
        }
        dir.insert(name.to_string(), Node::Dir(BTreeMap::new()));
        Ok(())
    }

    fn remove(&self, rel: &str) -> Result<(), FsError> {
        let (parent, name) = split_parent(rel).ok_or(FsError::Io)?;
        let mut root = self.root.lock();
        let parent_node = walk_mut(&mut root, &parent)?;
        let dir = match parent_node {
            Node::Dir(d) => d,
            _ => return Err(FsError::NotADir),
        };
        match dir.get(name) {
            None => Err(FsError::NotFound),
            Some(Node::Dir(child)) if !child.is_empty() => Err(FsError::Io), // ENOTEMPTY
            _ => {
                dir.remove(name);
                Ok(())
            }
        }
    }
}
