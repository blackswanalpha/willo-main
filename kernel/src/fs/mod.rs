//! VFS — pluggable mount table.
//!
//! M10 replaces the single-backend `Fs` enum with a `Vfs` that owns up to
//! `MAX_MOUNTS` mounts. Each mount is a `Box<dyn FileSystem>` keyed by an
//! absolute path prefix (`""` is root, `"/tmp"`, `"/dev"`, …). Path resolution
//! is **longest-prefix-match** on `/`-aligned boundaries.
//!
//! Backends provide read + (optional) write surfaces through the
//! [`FileSystem`] trait. Default trait impls return `FsError::Unsupported`,
//! so a read-only FS only needs to implement the read trio + `backing()`.

pub mod dev;
pub mod fat;
pub mod ram;
pub mod tmp;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Debug, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    NotADir,
    IsADir,
    /// Disk I/O or filesystem-format failure.
    Io,
    /// Operation is not implemented for this filesystem (e.g. `write_file` on
    /// a read-only mount). Distinct from `Io` so the shell can map it to
    /// "read-only filesystem" instead of a generic I/O error.
    Unsupported,
}

/// Largest number of simultaneous mounts. Plenty of headroom for M10's
/// `/`, `/tmp`, `/dev` plus future `/proc`, `/sys`, `/mnt/*`.
pub const MAX_MOUNTS: usize = 8;

/// Per-backend operations the VFS layers on top of. All paths are **relative
/// to the mount point** and always start with `/` (so the root of a mount is
/// `"/"`, never `""`).
pub trait FileSystem: Send + Sync {
    fn read_file(&self, rel: &str) -> Result<Vec<u8>, FsError>;
    fn list_dir(&self, rel: &str) -> Result<Vec<String>, FsError>;
    fn is_dir(&self, rel: &str) -> bool;

    /// `true` if any of the write methods can succeed. Drives the shell's
    /// "read-only filesystem" diagnostic.
    fn writable(&self) -> bool {
        false
    }

    /// Human-readable backing-store description for the `mount` command.
    fn backing(&self) -> &'static str;

    fn create_file(&self, _rel: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn write_file(&self, _rel: &str, _data: &[u8]) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn append_file(&self, _rel: &str, _data: &[u8]) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn create_dir(&self, _rel: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn remove(&self, _rel: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }

    /// Offset-based read used by special files (e.g. `/dev/zero`). Default
    /// implementation falls through to `read_file` and slices the result —
    /// fine for in-memory and on-disk FS, overridden by `devfs`.
    fn read_at(&self, rel: &str, off: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let data = self.read_file(rel)?;
        let off = off as usize;
        if off >= data.len() {
            return Ok(0);
        }
        let n = (data.len() - off).min(buf.len());
        buf[..n].copy_from_slice(&data[off..off + n]);
        Ok(n)
    }

    fn write_at(&self, _rel: &str, _off: u64, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::Unsupported)
    }
}

struct Mount {
    /// Stored without trailing `/` (root mount stores `""`); `"/tmp"`, `"/dev"`.
    prefix: String,
    fs: Box<dyn FileSystem>,
}

pub struct Vfs {
    mounts: Vec<Mount>,
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

impl Vfs {
    pub fn new() -> Self {
        Self {
            mounts: Vec::with_capacity(MAX_MOUNTS),
        }
    }

    /// Mount `fs` at the given absolute `prefix`. The root mount uses `"/"`.
    /// Errors if the table is full, the prefix is malformed, or the prefix is
    /// already taken.
    pub fn mount(&mut self, prefix: &str, fs: Box<dyn FileSystem>) -> Result<(), FsError> {
        if self.mounts.len() >= MAX_MOUNTS {
            return Err(FsError::Unsupported); // ENOSPC for mount table
        }
        if !prefix.starts_with('/') {
            return Err(FsError::Io); // EINVAL
        }
        let canonical = canonical_prefix(prefix);
        if self.mounts.iter().any(|m| m.prefix == canonical) {
            return Err(FsError::Io); // EEXIST
        }
        self.mounts.push(Mount {
            prefix: canonical,
            fs,
        });
        Ok(())
    }

    pub fn unmount(&mut self, prefix: &str) -> Result<(), FsError> {
        let canonical = canonical_prefix(prefix);
        let pos = self
            .mounts
            .iter()
            .position(|m| m.prefix == canonical)
            .ok_or(FsError::NotFound)?;
        self.mounts.remove(pos);
        Ok(())
    }

    /// Iterate mounts in `(prefix, backing)` order — used by `mount` shell cmd.
    /// Returns `/` for the root mount instead of the stored empty string.
    pub fn mounts(&self) -> impl Iterator<Item = (&str, &'static str)> {
        self.mounts.iter().map(|m| {
            let p = if m.prefix.is_empty() {
                "/"
            } else {
                m.prefix.as_str()
            };
            (p, m.fs.backing())
        })
    }

    /// Longest-prefix match. Returns the matching FS and the path **relative
    /// to the mount root** (always starts with `/`). E.g. `/tmp/log` →
    /// `(tmpfs, "/log")`; `/etc/motd` → `(fatfs, "/etc/motd")`; `/tmp` →
    /// `(tmpfs, "/")`.
    fn resolve(&self, abs: &str) -> Result<(&dyn FileSystem, String), FsError> {
        if !abs.starts_with('/') {
            return Err(FsError::NotFound);
        }
        let mut best: Option<&Mount> = None;
        for m in &self.mounts {
            if m.prefix.is_empty() {
                if best.is_none() {
                    best = Some(m);
                }
            } else if abs == m.prefix
                || (abs.len() > m.prefix.len()
                    && abs.starts_with(&m.prefix)
                    && abs.as_bytes()[m.prefix.len()] == b'/')
            {
                if best.is_none_or(|b| b.prefix.len() < m.prefix.len()) {
                    best = Some(m);
                }
            }
        }
        let m = best.ok_or(FsError::NotFound)?;
        let rel = if m.prefix.is_empty() {
            abs.to_string()
        } else {
            let tail = &abs[m.prefix.len()..];
            if tail.is_empty() {
                "/".to_string()
            } else {
                tail.to_string()
            }
        };
        Ok((m.fs.as_ref(), rel))
    }

    pub fn read_file(&self, abs: &str) -> Result<Vec<u8>, FsError> {
        let (fs, rel) = self.resolve(abs)?;
        fs.read_file(&rel)
    }

    pub fn list_dir(&self, abs: &str) -> Result<Vec<String>, FsError> {
        let (fs, rel) = self.resolve(abs)?;
        let mut entries = fs.list_dir(&rel)?;
        // Surface direct child mount points as virtual entries so `ls /` shows
        // `tmp`, `dev` etc. without those mounts living inside the root FS.
        let parent = canonical_prefix(abs);
        for m in &self.mounts {
            if let Some(child) = direct_child(&parent, &m.prefix)
                && !entries.iter().any(|e| e == child)
            {
                entries.push(child.to_string());
            }
        }
        Ok(entries)
    }

    pub fn is_dir(&self, abs: &str) -> bool {
        let canonical = canonical_prefix(abs);
        // Any mount point is a dir.
        if self.mounts.iter().any(|m| m.prefix == canonical) {
            return true;
        }
        // Empty-root request — at least the root mount must exist for `is_dir`
        // of "/" to be true; that's covered by the mount-prefix check above
        // when root is mounted as "" (canonical of "/").
        match self.resolve(abs) {
            Ok((fs, rel)) => fs.is_dir(&rel),
            Err(_) => false,
        }
    }

    pub fn writable(&self, abs: &str) -> bool {
        match self.resolve(abs) {
            Ok((fs, _)) => fs.writable(),
            Err(_) => false,
        }
    }

    /// Human-readable backing for the FS that owns `abs`. Used by the shell
    /// in error messages so "touch /dev/foo" can say "read-only filesystem".
    pub fn backing(&self, abs: &str) -> &'static str {
        match self.resolve(abs) {
            Ok((fs, _)) => fs.backing(),
            Err(_) => "?",
        }
    }

    pub fn create_file(&self, abs: &str) -> Result<(), FsError> {
        let (fs, rel) = self.resolve(abs)?;
        fs.create_file(&rel)
    }

    pub fn write_file(&self, abs: &str, data: &[u8]) -> Result<(), FsError> {
        let (fs, rel) = self.resolve(abs)?;
        fs.write_file(&rel, data)
    }

    pub fn append_file(&self, abs: &str, data: &[u8]) -> Result<(), FsError> {
        let (fs, rel) = self.resolve(abs)?;
        fs.append_file(&rel, data)
    }

    pub fn create_dir(&self, abs: &str) -> Result<(), FsError> {
        let (fs, rel) = self.resolve(abs)?;
        fs.create_dir(&rel)
    }

    pub fn remove(&self, abs: &str) -> Result<(), FsError> {
        let (fs, rel) = self.resolve(abs)?;
        fs.remove(&rel)
    }

    pub fn read_at(&self, abs: &str, off: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let (fs, rel) = self.resolve(abs)?;
        fs.read_at(&rel, off, buf)
    }

    pub fn write_at(&self, abs: &str, off: u64, buf: &[u8]) -> Result<usize, FsError> {
        let (fs, rel) = self.resolve(abs)?;
        fs.write_at(&rel, off, buf)
    }
}

/// Build the M10 root filesystem: FAT32 (or RamFs fallback) at `/`, tmpfs at
/// `/tmp`, devfs at `/dev`. Logged to serial so a missing data disk is easy to
/// spot in test output.
pub fn mount_root() -> Vfs {
    let mut vfs = Vfs::new();

    // Root.
    let root: Box<dyn FileSystem> = match crate::ata::init() {
        Ok(()) => match fat::FatFs::mount() {
            Ok(fat) => {
                crate::serial_println!("[fs] mounted FAT32 on primary IDE slave");
                Box::new(fat)
            }
            Err(e) => {
                crate::serial_println!(
                    "[fs] FAT mount failed ({:?}); falling back to ramfs",
                    e
                );
                Box::new(ram::seed())
            }
        },
        Err(e) => {
            crate::serial_println!(
                "[fs] no data disk on primary IDE slave ({:?}); using ramfs",
                e
            );
            Box::new(ram::seed())
        }
    };
    vfs.mount("/", root).expect("root mount");
    vfs.mount("/tmp", Box::new(tmp::TmpFs::new()))
        .expect("tmpfs mount");
    vfs.mount("/dev", Box::new(dev::DevFs::new()))
        .expect("devfs mount");
    // `/ram` is always populated with the seed read-only tree — useful as a
    // scratch namespace independent of the data disk, and gives the VFS the
    // ≥4 simultaneous mounts called for in the M10 spec.
    vfs.mount("/ram", Box::new(ram::seed()))
        .expect("ramfs mount");

    vfs
}

/// Resolve `input` (which may be absolute or relative) against `cwd` into a
/// canonical absolute path. Handles `.`, `..`, and runs of `/`. Pure function.
pub fn normalize(cwd: &str, input: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    if !input.starts_with('/') {
        for s in cwd.split('/').filter(|s| !s.is_empty()) {
            stack.push(s);
        }
    }
    for s in input.split('/').filter(|s| !s.is_empty()) {
        match s {
            "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    if stack.is_empty() {
        return "/".to_string();
    }
    let mut out = String::with_capacity(input.len() + cwd.len());
    for s in &stack {
        out.push('/');
        out.push_str(s);
    }
    out
}

/// Canonicalise a mount prefix or directory path: trim trailing `/`, treat
/// `""` and `"/"` identically (both → `""`). Used so `Vfs::is_dir("/")` lines
/// up with the root-mount key.
fn canonical_prefix(p: &str) -> String {
    if p == "/" {
        return String::new();
    }
    p.trim_end_matches('/').to_string()
}

/// If `child_prefix` is a direct child mount of `parent` (e.g. parent `""`
/// and child `"/tmp"`), return the basename. Otherwise None.
fn direct_child<'a>(parent: &str, child_prefix: &'a str) -> Option<&'a str> {
    if child_prefix.is_empty() {
        return None;
    }
    let rest = if parent.is_empty() {
        child_prefix.strip_prefix('/')?
    } else {
        let after = child_prefix.strip_prefix(parent)?;
        after.strip_prefix('/')?
    };
    if rest.is_empty() || rest.contains('/') {
        None
    } else {
        Some(rest)
    }
}
