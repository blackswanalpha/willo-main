//! In-memory read-only fallback filesystem.
//!
//! The tree is built by `seed()` and lives entirely on the heap (directory
//! metadata) and in `.rodata` (file contents — `&'static [u8]` slices). A
//! small `Special` variant lets a few synthetic files (like `/proc/uptime`)
//! be evaluated each time they're read.
//!
//! M9 prefers a real disk-backed `FatFs` (`super::fat`) and only falls back
//! here if no data disk is attached or the FAT mount fails. The data model
//! and the surface API are deliberately a strict subset of `FatFs` so the
//! shell behaves the same either way.

use super::{FileSystem, FsError};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub enum Node {
    File(&'static [u8]),
    Special(fn() -> String),
    Dir(BTreeMap<String, Node>),
}

pub struct RamFs {
    root: Node,
}

impl RamFs {
    fn lookup(&self, abs: &str) -> Result<&Node, FsError> {
        let mut node = &self.root;
        for seg in abs.split('/').filter(|s| !s.is_empty()) {
            let entries = match node {
                Node::Dir(d) => d,
                _ => return Err(FsError::NotADir),
            };
            node = entries.get(seg).ok_or(FsError::NotFound)?;
        }
        Ok(node)
    }

    pub fn read_file(&self, abs: &str) -> Result<Vec<u8>, FsError> {
        match self.lookup(abs)? {
            Node::File(b) => Ok(b.to_vec()),
            Node::Special(f) => Ok(f().into_bytes()),
            Node::Dir(_) => Err(FsError::IsADir),
        }
    }

    pub fn list_dir(&self, abs: &str) -> Result<Vec<String>, FsError> {
        match self.lookup(abs)? {
            Node::Dir(d) => Ok(d.keys().cloned().collect()),
            _ => Err(FsError::NotADir),
        }
    }

    pub fn is_dir(&self, abs: &str) -> bool {
        matches!(self.lookup(abs), Ok(Node::Dir(_)))
    }
}

impl FileSystem for RamFs {
    fn read_file(&self, rel: &str) -> Result<Vec<u8>, FsError> {
        RamFs::read_file(self, rel)
    }

    fn list_dir(&self, rel: &str) -> Result<Vec<String>, FsError> {
        RamFs::list_dir(self, rel)
    }

    fn is_dir(&self, rel: &str) -> bool {
        RamFs::is_dir(self, rel)
    }

    fn backing(&self) -> &'static str {
        "ramfs (no data disk)"
    }
}

/// Build the seed filesystem used as a fallback when no FAT data disk is
/// available. Files live in `.rodata`; `Special` callbacks may allocate.
pub fn seed() -> RamFs {
    let mut root: BTreeMap<String, Node> = BTreeMap::new();
    root.insert(
        "welcome.txt".to_string(),
        Node::File(b"Welcome to Willo!\nType `help`.\n"),
    );

    let mut etc: BTreeMap<String, Node> = BTreeMap::new();
    etc.insert("version".to_string(), Node::File(b"willo 0.9 (M9)\n"));
    etc.insert(
        "motd".to_string(),
        Node::File(b"the kernel that fits in your head\n"),
    );
    root.insert("etc".to_string(), Node::Dir(etc));

    let mut proc_dir: BTreeMap<String, Node> = BTreeMap::new();
    proc_dir.insert("uptime".to_string(), Node::Special(uptime_text));
    root.insert("proc".to_string(), Node::Dir(proc_dir));

    root.insert("bin".to_string(), Node::Dir(BTreeMap::new()));

    RamFs {
        root: Node::Dir(root),
    }
}

fn uptime_text() -> String {
    format!("{} ticks\n", crate::interrupts::ticks())
}
