//! Device filesystem.
//!
//! `/dev` exposes a small static set of pseudo-files that bridge to kernel
//! subsystems: the framebuffer, the discard sink, an infinite-zero source,
//! the UART, and the raw ATA disk. The node table is `&'static` because the
//! devices themselves are global singletons (FB writer, serial port, ATA
//! drive); registering them is just naming them.
//!
//! Generic `read_file` / `list_dir` keep the shell honest (`cat /dev/zero`
//! returns a fixed-size block, `cat /dev/null` returns empty), and
//! `read_at` / `write_at` are the streaming API the M10 devfs test uses.

use super::{FileSystem, FsError};
use crate::block::{BLOCK_SIZE, BlockDevice};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub trait DevNode: Send + Sync {
    fn read(&self, off: u64, buf: &mut [u8]) -> Result<usize, FsError>;
    fn write(&self, off: u64, buf: &[u8]) -> Result<usize, FsError>;
}

struct ConsoleNode;
struct NullNode;
struct ZeroNode;
struct Serial0Node;
struct Disk0Node;

impl DevNode for ConsoleNode {
    fn read(&self, _off: u64, _buf: &mut [u8]) -> Result<usize, FsError> {
        Err(FsError::Unsupported)
    }
    fn write(&self, _off: u64, buf: &[u8]) -> Result<usize, FsError> {
        // Best-effort UTF-8 print; non-utf8 bytes pass through as raw chars.
        let s = core::str::from_utf8(buf).unwrap_or("");
        crate::print!("{}", s);
        Ok(buf.len())
    }
}

impl DevNode for NullNode {
    fn read(&self, _off: u64, _buf: &mut [u8]) -> Result<usize, FsError> {
        Ok(0)
    }
    fn write(&self, _off: u64, buf: &[u8]) -> Result<usize, FsError> {
        Ok(buf.len())
    }
}

impl DevNode for ZeroNode {
    fn read(&self, _off: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        for b in buf.iter_mut() {
            *b = 0;
        }
        Ok(buf.len())
    }
    fn write(&self, _off: u64, buf: &[u8]) -> Result<usize, FsError> {
        Ok(buf.len())
    }
}

impl DevNode for Serial0Node {
    fn read(&self, _off: u64, _buf: &mut [u8]) -> Result<usize, FsError> {
        // No buffered input queue (M10) — readers always see EOF.
        Ok(0)
    }
    fn write(&self, _off: u64, buf: &[u8]) -> Result<usize, FsError> {
        if let Some(port) = crate::serial::SERIAL1.get() {
            let mut p = port.lock();
            for &b in buf {
                p.send(b);
            }
            Ok(buf.len())
        } else {
            Err(FsError::Io)
        }
    }
}

impl DevNode for Disk0Node {
    fn read(&self, off: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        if buf.is_empty() {
            return Ok(0);
        }
        if off % BLOCK_SIZE as u64 != 0 || buf.len() % BLOCK_SIZE != 0 {
            return Err(FsError::Io); // alignment-only for M10
        }
        let mut drive = crate::ata::DRIVE.lock();
        let blocks = buf.len() / BLOCK_SIZE;
        let start_lba = off / BLOCK_SIZE as u64;
        for i in 0..blocks {
            let dst = &mut buf[i * BLOCK_SIZE..(i + 1) * BLOCK_SIZE];
            drive
                .read_block(start_lba + i as u64, dst)
                .map_err(|_| FsError::Io)?;
        }
        Ok(buf.len())
    }

    fn write(&self, off: u64, buf: &[u8]) -> Result<usize, FsError> {
        if buf.is_empty() {
            return Ok(0);
        }
        if off % BLOCK_SIZE as u64 != 0 || buf.len() % BLOCK_SIZE != 0 {
            return Err(FsError::Io);
        }
        let mut drive = crate::ata::DRIVE.lock();
        let blocks = buf.len() / BLOCK_SIZE;
        let start_lba = off / BLOCK_SIZE as u64;
        for i in 0..blocks {
            let src = &buf[i * BLOCK_SIZE..(i + 1) * BLOCK_SIZE];
            drive
                .write_block(start_lba + i as u64, src)
                .map_err(|_| FsError::Io)?;
        }
        Ok(buf.len())
    }
}

const NODES: &[(&str, &dyn DevNode)] = &[
    ("console", &ConsoleNode),
    ("null", &NullNode),
    ("zero", &ZeroNode),
    ("serial0", &Serial0Node),
    ("disk0", &Disk0Node),
];

/// `cat /dev/zero` budget — keeps the convenience whole-file read finite.
const ZERO_READ_FILE_BYTES: usize = 4096;

pub struct DevFs;

impl DevFs {
    pub fn new() -> Self {
        Self
    }

    fn node(name: &str) -> Option<&'static dyn DevNode> {
        NODES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, node)| *node)
    }
}

impl Default for DevFs {
    fn default() -> Self {
        Self::new()
    }
}

/// Strip the leading `/` and reject paths with intermediate slashes — devfs
/// is a flat namespace.
fn flat_name(rel: &str) -> Result<&str, FsError> {
    let trimmed = rel.strip_prefix('/').unwrap_or(rel);
    if trimmed.contains('/') {
        return Err(FsError::NotFound);
    }
    Ok(trimmed)
}

impl FileSystem for DevFs {
    fn read_file(&self, rel: &str) -> Result<Vec<u8>, FsError> {
        let name = flat_name(rel)?;
        if name.is_empty() {
            return Err(FsError::IsADir);
        }
        match name {
            "null" => Ok(Vec::new()),
            "zero" => Ok(vec![0u8; ZERO_READ_FILE_BYTES]),
            "console" | "serial0" | "disk0" => Err(FsError::Unsupported),
            _ => Err(FsError::NotFound),
        }
    }

    fn list_dir(&self, rel: &str) -> Result<Vec<String>, FsError> {
        let name = flat_name(rel)?;
        if !name.is_empty() {
            return Err(FsError::NotADir);
        }
        Ok(NODES.iter().map(|(n, _)| (*n).to_string()).collect())
    }

    fn is_dir(&self, rel: &str) -> bool {
        match flat_name(rel) {
            Ok("") => true,
            _ => false,
        }
    }

    fn writable(&self) -> bool {
        true
    }

    fn backing(&self) -> &'static str {
        "devfs"
    }

    fn read_at(&self, rel: &str, off: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let name = flat_name(rel)?;
        let node = Self::node(name).ok_or(FsError::NotFound)?;
        node.read(off, buf)
    }

    fn write_at(&self, rel: &str, off: u64, buf: &[u8]) -> Result<usize, FsError> {
        let name = flat_name(rel)?;
        let node = Self::node(name).ok_or(FsError::NotFound)?;
        node.write(off, buf)
    }

    fn write_file(&self, rel: &str, data: &[u8]) -> Result<(), FsError> {
        let n = self.write_at(rel, 0, data)?;
        if n == data.len() {
            Ok(())
        } else {
            Err(FsError::Io)
        }
    }
}
