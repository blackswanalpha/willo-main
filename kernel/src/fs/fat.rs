//! Hand-rolled FAT32 reader/writer.
//!
//! Mounts the FAT volume on the primary IDE slave (see `crate::ata`) and
//! exposes `read_file` / `list_dir` / `is_dir` plus M10's full write surface
//! (`create_file`, `write_file`, `append_file`, `create_dir`, `remove`).
//!
//! M10 hardening notes:
//! - Every FAT mutation is mirrored to *all* `num_fats` copies so
//!   `fsck.fat -nv` is clean.
//! - The FsInfo sector (sector 1) is updated on every `allocate_chain` /
//!   `free_chain` so the free-cluster summary stays consistent.
//!
//! Limitations (intentional):
//! - 8.3 short names only — long-filename (LFN) entries are skipped on read
//!   and never emitted on write. The host runner and shell only emit names
//!   that already fit 8.3 (lowercase ASCII, ≤8 chars name + ≤3 chars ext).
//! - The volume label, `.`, and `..` directory entries are filtered from
//!   `list_dir`; `normalize()` already handles `.`/`..` in paths.
//! - Single FAT volume on the primary IDE slave; no MBR/partition-table walk
//!   (the host runner formats the disk as a single bare FAT volume).

use super::{FileSystem, FsError};
use crate::ata::DRIVE;
use crate::block::{BLOCK_SIZE, BlockDevice, BlockError};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_HIDDEN: u8 = 0x02;
const ATTR_SYSTEM: u8 = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_LONG_NAME: u8 = ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_SYSTEM | ATTR_VOLUME_ID;

const EOC_THRESHOLD: u32 = 0x0FFFFFF8;
const CLUSTER_MASK: u32 = 0x0FFFFFFF;

pub struct FatFs {
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub reserved_sectors: u32,
    pub num_fats: u32,
    pub sectors_per_fat: u32,
    pub root_cluster: u32,
    pub fat_start_lba: u32,
    pub data_start_lba: u32,
}

#[derive(Debug, Clone)]
struct DirEntry {
    name: String,
    is_dir: bool,
    cluster: u32,
    size: u32,
}

impl From<BlockError> for FsError {
    fn from(_: BlockError) -> Self {
        FsError::Io
    }
}

impl FatFs {
    /// Read sector 0 (the BPB) and validate the FAT32 layout.
    pub fn mount() -> Result<Self, FsError> {
        let mut buf = [0u8; BLOCK_SIZE];
        DRIVE.lock().read_block(0, &mut buf)?;

        let bytes_per_sector = u16::from_le_bytes([buf[0x0B], buf[0x0C]]) as u32;
        let sectors_per_cluster = buf[0x0D] as u32;
        let reserved_sectors = u16::from_le_bytes([buf[0x0E], buf[0x0F]]) as u32;
        let num_fats = buf[0x10] as u32;
        let sectors_per_fat = u32::from_le_bytes([buf[0x24], buf[0x25], buf[0x26], buf[0x27]]);
        let root_cluster = u32::from_le_bytes([buf[0x2C], buf[0x2D], buf[0x2E], buf[0x2F]]);

        if bytes_per_sector != BLOCK_SIZE as u32 || sectors_per_cluster == 0 || num_fats == 0 {
            return Err(FsError::Io);
        }

        let fat_start_lba = reserved_sectors;
        let data_start_lba = reserved_sectors + num_fats * sectors_per_fat;

        Ok(Self {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            num_fats,
            sectors_per_fat,
            root_cluster,
            fat_start_lba,
            data_start_lba,
        })
    }

    pub fn read_file(&self, abs: &str) -> Result<Vec<u8>, FsError> {
        let entry = self.lookup(abs)?;
        if entry.is_dir {
            return Err(FsError::IsADir);
        }
        if entry.size == 0 || entry.cluster < 2 {
            return Ok(Vec::new());
        }
        let mut out = self.read_chain(entry.cluster, entry.size as usize)?;
        out.truncate(entry.size as usize);
        Ok(out)
    }

    pub fn list_dir(&self, abs: &str) -> Result<Vec<String>, FsError> {
        let entry = self.lookup(abs)?;
        if !entry.is_dir {
            return Err(FsError::NotADir);
        }
        let cluster = if abs == "/" {
            self.root_cluster
        } else {
            entry.cluster
        };
        let entries = self.read_dir(cluster)?;
        Ok(entries.into_iter().map(|e| e.name).collect())
    }

    pub fn is_dir(&self, abs: &str) -> bool {
        match self.lookup(abs) {
            Ok(e) => e.is_dir,
            Err(_) => false,
        }
    }

    fn lookup(&self, abs: &str) -> Result<DirEntry, FsError> {
        if abs == "/" || abs.is_empty() {
            return Ok(DirEntry {
                name: "/".to_string(),
                is_dir: true,
                cluster: self.root_cluster,
                size: 0,
            });
        }
        let mut current_cluster = self.root_cluster;
        let mut found: Option<DirEntry> = None;
        for (i, seg) in abs.split('/').filter(|s| !s.is_empty()).enumerate() {
            if i > 0 {
                let f = found.as_ref().ok_or(FsError::NotFound)?;
                if !f.is_dir {
                    return Err(FsError::NotADir);
                }
                current_cluster = f.cluster;
            }
            let entries = self.read_dir(current_cluster)?;
            found = entries
                .into_iter()
                .find(|e| e.name.eq_ignore_ascii_case(seg));
            if found.is_none() {
                return Err(FsError::NotFound);
            }
        }
        found.ok_or(FsError::NotFound)
    }

    fn read_dir(&self, cluster: u32) -> Result<Vec<DirEntry>, FsError> {
        let bytes = self.read_chain(cluster, usize::MAX)?;
        let mut out = Vec::new();
        for chunk in bytes.chunks_exact(32) {
            // Ordering: end-of-dir terminates everything.
            if chunk[0] == 0x00 {
                break;
            }
            // Deleted, volume label, or LFN — skip.
            if chunk[0] == 0xE5
                || chunk[11] & ATTR_LONG_NAME == ATTR_LONG_NAME
                || chunk[11] & ATTR_VOLUME_ID != 0
            {
                continue;
            }
            let name = decode_short_name(chunk);
            if name == "." || name == ".." {
                continue;
            }
            let cluster = ((u16::from_le_bytes([chunk[20], chunk[21]]) as u32) << 16)
                | (u16::from_le_bytes([chunk[26], chunk[27]]) as u32);
            let size = u32::from_le_bytes([chunk[28], chunk[29], chunk[30], chunk[31]]);
            let is_dir = chunk[11] & ATTR_DIRECTORY != 0;
            out.push(DirEntry {
                name,
                is_dir,
                cluster,
                size,
            });
        }
        Ok(out)
    }

    /// Walk a cluster chain, accumulating data up to `max_bytes`. The result's
    /// length is always a whole number of clusters (caller truncates to the
    /// known file size).
    fn read_chain(&self, start: u32, max_bytes: usize) -> Result<Vec<u8>, FsError> {
        let cluster_bytes = (self.sectors_per_cluster * self.bytes_per_sector) as usize;
        let mut out: Vec<u8> = Vec::new();
        let mut cluster = start;
        while cluster >= 2 && cluster < EOC_THRESHOLD {
            let lba = self.cluster_to_lba(cluster);
            let mut buf = vec![0u8; cluster_bytes];
            for sec in 0..self.sectors_per_cluster as u64 {
                let off = (sec as usize) * BLOCK_SIZE;
                DRIVE
                    .lock()
                    .read_block(lba as u64 + sec, &mut buf[off..off + BLOCK_SIZE])?;
            }
            out.extend_from_slice(&buf);
            if out.len() >= max_bytes {
                break;
            }
            cluster = self.next_cluster(cluster)?;
        }
        Ok(out)
    }

    fn cluster_to_lba(&self, cluster: u32) -> u32 {
        self.data_start_lba + (cluster - 2) * self.sectors_per_cluster
    }

    fn next_cluster(&self, cluster: u32) -> Result<u32, FsError> {
        let fat_offset = cluster * 4;
        let fat_sector = self.fat_start_lba + fat_offset / self.bytes_per_sector;
        let in_sector = (fat_offset % self.bytes_per_sector) as usize;
        let mut buf = [0u8; BLOCK_SIZE];
        DRIVE.lock().read_block(fat_sector as u64, &mut buf)?;
        let raw = u32::from_le_bytes([
            buf[in_sector],
            buf[in_sector + 1],
            buf[in_sector + 2],
            buf[in_sector + 3],
        ]);
        Ok(raw & CLUSTER_MASK)
    }

    // ---- M10 write path ----

    /// Create an empty file (or truncate an existing one to zero length).
    pub fn create_file(&self, abs: &str) -> Result<(), FsError> {
        let (parent, name) = split_parent(abs)?;
        if !self.is_dir(&parent) {
            return Err(FsError::NotADir);
        }
        let parent_cluster = self.cluster_for(&parent)?;
        if self.find_in_dir(parent_cluster, &name)?.is_some() {
            // Already exists — truncate. M10's UX: `touch` is idempotent.
            self.truncate_file(parent_cluster, &name)?;
            return Ok(());
        }
        self.add_dir_entry(parent_cluster, &name, 0, 0, 0)?;
        Ok(())
    }

    /// Append `data` at the tail of `abs`, creating the file if it does not
    /// exist. Implemented as read-existing → extend → rewrite, which is fine
    /// at M10's volume scale (≤64 MiB) and matches the work10 task list's
    /// "create / append / delete" coverage requirement.
    pub fn append_file(&self, abs: &str, data: &[u8]) -> Result<(), FsError> {
        let existing = match self.read_file(abs) {
            Ok(b) => b,
            Err(FsError::NotFound) => Vec::new(),
            Err(e) => return Err(e),
        };
        let mut combined = existing;
        combined.extend_from_slice(data);
        self.write_file(abs, &combined)
    }

    /// Overwrite (or create) `abs` with `data`. Reuses or extends the existing
    /// cluster chain as needed.
    pub fn write_file(&self, abs: &str, data: &[u8]) -> Result<(), FsError> {
        let (parent, name) = split_parent(abs)?;
        if !self.is_dir(&parent) {
            return Err(FsError::NotADir);
        }
        let parent_cluster = self.cluster_for(&parent)?;

        // Remove old chain (if any) — easier than trying to in-place rewrite.
        if let Some(old) = self.find_in_dir(parent_cluster, &name)? {
            if old.is_dir {
                return Err(FsError::IsADir);
            }
            if old.cluster >= 2 {
                self.free_chain(old.cluster)?;
            }
            self.delete_dir_entry(parent_cluster, &name)?;
        }

        let cluster_bytes = (self.sectors_per_cluster * self.bytes_per_sector) as usize;
        let first_cluster = if data.is_empty() {
            0
        } else {
            let n_clusters = data.len().div_ceil(cluster_bytes) as u32;
            let head = self.allocate_chain(n_clusters)?;
            self.write_chain(head, data)?;
            head
        };
        self.add_dir_entry(parent_cluster, &name, 0x20, first_cluster, data.len() as u32)?;
        Ok(())
    }

    /// Create an empty directory at `abs`.
    pub fn create_dir(&self, abs: &str) -> Result<(), FsError> {
        let (parent, name) = split_parent(abs)?;
        if !self.is_dir(&parent) {
            return Err(FsError::NotADir);
        }
        let parent_cluster = self.cluster_for(&parent)?;
        if self.find_in_dir(parent_cluster, &name)?.is_some() {
            return Err(FsError::Io); // EEXIST
        }
        // Allocate one cluster for the new directory's content.
        let cluster = self.allocate_chain(1)?;
        // Write `.` and `..` entries inside the new cluster, rest zeros.
        let cluster_bytes = (self.sectors_per_cluster * self.bytes_per_sector) as usize;
        let mut buf = vec![0u8; cluster_bytes];
        write_short_entry(&mut buf[0..32], ".", ATTR_DIRECTORY, cluster, 0);
        let parent_for_dotdot = if parent == "/" { 0 } else { parent_cluster };
        write_short_entry(&mut buf[32..64], "..", ATTR_DIRECTORY, parent_for_dotdot, 0);
        self.write_cluster(cluster, &buf)?;
        // Link the new directory in its parent.
        self.add_dir_entry(parent_cluster, &name, ATTR_DIRECTORY, cluster, 0)?;
        Ok(())
    }

    /// Remove a file or empty directory.
    pub fn remove(&self, abs: &str) -> Result<(), FsError> {
        let (parent, name) = split_parent(abs)?;
        let parent_cluster = self.cluster_for(&parent)?;
        let entry = self
            .find_in_dir(parent_cluster, &name)?
            .ok_or(FsError::NotFound)?;
        if entry.is_dir {
            // Make sure the directory is empty (only `.`/`..` allowed).
            let kids = self.read_dir(entry.cluster)?;
            if !kids.is_empty() {
                return Err(FsError::Io); // ENOTEMPTY
            }
        }
        if entry.cluster >= 2 {
            self.free_chain(entry.cluster)?;
        }
        self.delete_dir_entry(parent_cluster, &name)?;
        Ok(())
    }

    /// Resolve a path to its directory's first cluster (must be a directory).
    fn cluster_for(&self, abs: &str) -> Result<u32, FsError> {
        if abs == "/" {
            return Ok(self.root_cluster);
        }
        let entry = self.lookup(abs)?;
        if !entry.is_dir {
            return Err(FsError::NotADir);
        }
        Ok(entry.cluster)
    }

    fn find_in_dir(&self, cluster: u32, name: &str) -> Result<Option<DirEntry>, FsError> {
        let entries = self.read_dir(cluster)?;
        Ok(entries.into_iter().find(|e| e.name.eq_ignore_ascii_case(name)))
    }

    fn truncate_file(&self, parent_cluster: u32, name: &str) -> Result<(), FsError> {
        let entry = self
            .find_in_dir(parent_cluster, name)?
            .ok_or(FsError::NotFound)?;
        if entry.is_dir {
            return Err(FsError::IsADir);
        }
        if entry.cluster >= 2 {
            self.free_chain(entry.cluster)?;
        }
        // Rewrite the entry with cluster=0, size=0.
        self.delete_dir_entry(parent_cluster, name)?;
        self.add_dir_entry(parent_cluster, name, 0x20, 0, 0)?;
        Ok(())
    }

    /// Allocate `count` clusters, link them as a chain, return the head.
    fn allocate_chain(&self, count: u32) -> Result<u32, FsError> {
        if count == 0 {
            return Ok(0);
        }
        let mut head = 0u32;
        let mut prev = 0u32;
        let mut last = 0u32;
        for _ in 0..count {
            let c = self.find_free_cluster()?;
            // Mark as EOC immediately so a concurrent allocation (we're
            // single-threaded but defensive) doesn't reuse it. Then patch
            // `prev`'s link to point here.
            self.write_fat_entry(c, EOC_THRESHOLD | 0xF)?;
            self.zero_cluster(c)?;
            if prev == 0 {
                head = c;
            } else {
                self.write_fat_entry(prev, c)?;
            }
            prev = c;
            last = c;
        }
        let _ = self.update_fsinfo(-(count as i32), last);
        Ok(head)
    }

    /// Walk the FAT looking for the first cluster whose entry is 0 (free).
    fn find_free_cluster(&self) -> Result<u32, FsError> {
        let total_clusters = (self.sectors_per_fat * self.bytes_per_sector) / 4;
        let mut buf = [0u8; BLOCK_SIZE];
        let mut current_sector = u32::MAX;
        for c in 2..total_clusters {
            let fat_offset = c * 4;
            let fat_sector = self.fat_start_lba + fat_offset / self.bytes_per_sector;
            let in_sector = (fat_offset % self.bytes_per_sector) as usize;
            if fat_sector != current_sector {
                DRIVE.lock().read_block(fat_sector as u64, &mut buf)?;
                current_sector = fat_sector;
            }
            let raw = u32::from_le_bytes([
                buf[in_sector],
                buf[in_sector + 1],
                buf[in_sector + 2],
                buf[in_sector + 3],
            ]) & CLUSTER_MASK;
            if raw == 0 {
                return Ok(c);
            }
        }
        Err(FsError::Io)
    }

    /// Write `value` (low 28 bits) into the FAT entry for `cluster`. Mirrored
    /// across every FAT copy on the volume so `fsck.fat -nv` stays clean.
    fn write_fat_entry(&self, cluster: u32, value: u32) -> Result<(), FsError> {
        let fat_offset = cluster * 4;
        let in_sector = (fat_offset % self.bytes_per_sector) as usize;
        for i in 0..self.num_fats {
            let fat_sector =
                self.fat_start_lba + i * self.sectors_per_fat + fat_offset / self.bytes_per_sector;
            let mut buf = [0u8; BLOCK_SIZE];
            DRIVE.lock().read_block(fat_sector as u64, &mut buf)?;
            let masked = (value & CLUSTER_MASK)
                | (u32::from_le_bytes([
                    buf[in_sector],
                    buf[in_sector + 1],
                    buf[in_sector + 2],
                    buf[in_sector + 3],
                ]) & !CLUSTER_MASK);
            buf[in_sector..in_sector + 4].copy_from_slice(&masked.to_le_bytes());
            DRIVE.lock().write_block(fat_sector as u64, &buf)?;
        }
        Ok(())
    }

    /// Update FsInfo (sector 1) free-cluster count and next-free hint after
    /// allocating (`delta_free` negative) or freeing (`delta_free` positive)
    /// `count` clusters. We don't fail on a stale FsInfo signature — older
    /// `mkfs.fat` versions occasionally leave a missing trail magic; the
    /// fsck warning would be cosmetic, and we'd rather silently noop than
    /// abort a write.
    fn update_fsinfo(&self, delta_free: i32, last_alloc: u32) -> Result<(), FsError> {
        let mut buf = [0u8; BLOCK_SIZE];
        DRIVE.lock().read_block(1, &mut buf)?;
        // Lead signature 0x41615252 at bytes 0..4; struct signature 0x61417272
        // at 484..488. Skip silently if unset.
        let lead = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let strct = u32::from_le_bytes([buf[484], buf[485], buf[486], buf[487]]);
        if lead != 0x4161_5252 || strct != 0x6141_7272 {
            return Ok(());
        }
        let mut free = u32::from_le_bytes([buf[0x1E8], buf[0x1E9], buf[0x1EA], buf[0x1EB]]);
        if free != 0xFFFF_FFFF {
            free = (free as i64 + delta_free as i64).max(0) as u32;
            buf[0x1E8..0x1EC].copy_from_slice(&free.to_le_bytes());
        }
        if last_alloc != 0 {
            buf[0x1EC..0x1F0].copy_from_slice(&last_alloc.to_le_bytes());
        }
        DRIVE.lock().write_block(1, &buf)?;
        Ok(())
    }

    fn free_chain(&self, start: u32) -> Result<(), FsError> {
        let mut cluster = start;
        let mut freed = 0i32;
        while cluster >= 2 && cluster < EOC_THRESHOLD {
            let next = self.next_cluster(cluster)?;
            self.write_fat_entry(cluster, 0)?;
            cluster = next;
            freed += 1;
        }
        if freed > 0 {
            let _ = self.update_fsinfo(freed, 0);
        }
        Ok(())
    }

    fn write_chain(&self, head: u32, data: &[u8]) -> Result<(), FsError> {
        let cluster_bytes = (self.sectors_per_cluster * self.bytes_per_sector) as usize;
        let mut cluster = head;
        let mut offset = 0usize;
        while offset < data.len() && cluster >= 2 && cluster < EOC_THRESHOLD {
            let mut buf = vec![0u8; cluster_bytes];
            let chunk_end = (offset + cluster_bytes).min(data.len());
            buf[..chunk_end - offset].copy_from_slice(&data[offset..chunk_end]);
            self.write_cluster(cluster, &buf)?;
            offset = chunk_end;
            if offset >= data.len() {
                break;
            }
            cluster = self.next_cluster(cluster)?;
        }
        Ok(())
    }

    fn write_cluster(&self, cluster: u32, data: &[u8]) -> Result<(), FsError> {
        let lba = self.cluster_to_lba(cluster) as u64;
        for sec in 0..self.sectors_per_cluster as u64 {
            let off = (sec as usize) * BLOCK_SIZE;
            DRIVE.lock().write_block(lba + sec, &data[off..off + BLOCK_SIZE])?;
        }
        Ok(())
    }

    fn zero_cluster(&self, cluster: u32) -> Result<(), FsError> {
        let cluster_bytes = (self.sectors_per_cluster * self.bytes_per_sector) as usize;
        let buf = vec![0u8; cluster_bytes];
        self.write_cluster(cluster, &buf)
    }

    /// Find a free directory-entry slot in `parent_cluster` and write a fresh
    /// 32-byte short-name entry there. Grows the directory by appending a new
    /// cluster only if the existing chain is fully populated.
    fn add_dir_entry(
        &self,
        parent_cluster: u32,
        name: &str,
        attrs: u8,
        cluster: u32,
        size: u32,
    ) -> Result<(), FsError> {
        let cluster_bytes = (self.sectors_per_cluster * self.bytes_per_sector) as usize;
        let mut chain_cluster = parent_cluster;
        loop {
            let lba = self.cluster_to_lba(chain_cluster);
            for sec in 0..self.sectors_per_cluster as u64 {
                let mut buf = [0u8; BLOCK_SIZE];
                DRIVE.lock().read_block(lba as u64 + sec, &mut buf)?;
                for slot_in_sec in 0..(BLOCK_SIZE / 32) {
                    let off = slot_in_sec * 32;
                    let first = buf[off];
                    if first == 0x00 || first == 0xE5 {
                        write_short_entry(
                            &mut buf[off..off + 32],
                            name,
                            attrs,
                            cluster,
                            size,
                        );
                        DRIVE.lock().write_block(lba as u64 + sec, &buf)?;
                        return Ok(());
                    }
                }
            }
            // No free slot in this cluster — follow the chain or grow it.
            let next = self.next_cluster(chain_cluster)?;
            if next < 2 || next >= EOC_THRESHOLD {
                // Grow: allocate one new cluster, link, zero, restart in it.
                let new_cluster = self.find_free_cluster()?;
                self.write_fat_entry(new_cluster, EOC_THRESHOLD | 0xF)?;
                self.zero_cluster(new_cluster)?;
                self.write_fat_entry(chain_cluster, new_cluster)?;
                let _ = self.update_fsinfo(-1, new_cluster);
                chain_cluster = new_cluster;
                let _ = cluster_bytes; // silence unused-let warning on growth path
            } else {
                chain_cluster = next;
            }
        }
    }

    /// Mark the directory entry for `name` in `parent_cluster` as deleted by
    /// writing 0xE5 in byte 0. Does NOT free the file's cluster chain.
    fn delete_dir_entry(&self, parent_cluster: u32, name: &str) -> Result<(), FsError> {
        let mut chain_cluster = parent_cluster;
        loop {
            let lba = self.cluster_to_lba(chain_cluster);
            for sec in 0..self.sectors_per_cluster as u64 {
                let mut buf = [0u8; BLOCK_SIZE];
                DRIVE.lock().read_block(lba as u64 + sec, &mut buf)?;
                for slot_in_sec in 0..(BLOCK_SIZE / 32) {
                    let off = slot_in_sec * 32;
                    let chunk = &buf[off..off + 32];
                    if chunk[0] == 0x00 {
                        return Ok(());
                    }
                    if chunk[0] == 0xE5
                        || chunk[11] & ATTR_LONG_NAME == ATTR_LONG_NAME
                        || chunk[11] & ATTR_VOLUME_ID != 0
                    {
                        continue;
                    }
                    let entry_name = decode_short_name(chunk);
                    if entry_name.eq_ignore_ascii_case(name) {
                        buf[off] = 0xE5;
                        DRIVE.lock().write_block(lba as u64 + sec, &buf)?;
                        return Ok(());
                    }
                }
            }
            let next = self.next_cluster(chain_cluster)?;
            if next < 2 || next >= EOC_THRESHOLD {
                return Err(FsError::NotFound);
            }
            chain_cluster = next;
        }
    }
}

impl FileSystem for FatFs {
    fn read_file(&self, rel: &str) -> Result<Vec<u8>, FsError> {
        FatFs::read_file(self, rel)
    }

    fn list_dir(&self, rel: &str) -> Result<Vec<String>, FsError> {
        FatFs::list_dir(self, rel)
    }

    fn is_dir(&self, rel: &str) -> bool {
        FatFs::is_dir(self, rel)
    }

    fn writable(&self) -> bool {
        true
    }

    fn backing(&self) -> &'static str {
        "fat32 on /dev/ata1"
    }

    fn create_file(&self, rel: &str) -> Result<(), FsError> {
        FatFs::create_file(self, rel)
    }

    fn write_file(&self, rel: &str, data: &[u8]) -> Result<(), FsError> {
        FatFs::write_file(self, rel, data)
    }

    fn append_file(&self, rel: &str, data: &[u8]) -> Result<(), FsError> {
        FatFs::append_file(self, rel, data)
    }

    fn create_dir(&self, rel: &str) -> Result<(), FsError> {
        FatFs::create_dir(self, rel)
    }

    fn remove(&self, rel: &str) -> Result<(), FsError> {
        FatFs::remove(self, rel)
    }
}

/// Split `/foo/bar/baz` into `("/foo/bar", "baz")`. Errors on the empty path
/// or on plain `/`.
fn split_parent(abs: &str) -> Result<(String, String), FsError> {
    let trimmed = abs.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(FsError::Io);
    }
    let idx = trimmed.rfind('/').ok_or(FsError::Io)?;
    let parent = if idx == 0 {
        "/".to_string()
    } else {
        trimmed[..idx].to_string()
    };
    let name = trimmed[idx + 1..].to_string();
    if name.is_empty() {
        return Err(FsError::Io);
    }
    Ok((parent, name))
}

/// Encode `name` (lowercase 8.3) into a 32-byte directory entry at `slot`.
/// FAT 8.3 names are uppercase ASCII, space-padded. We store names uppercase
/// (no NT-flags) and rely on `decode_short_name` to lowercase on read.
fn write_short_entry(slot: &mut [u8], name: &str, attrs: u8, cluster: u32, size: u32) {
    debug_assert_eq!(slot.len(), 32);
    slot.fill(0);
    let (basename, ext) = match name {
        "." => (".", ""),
        ".." => ("..", ""),
        n => match n.rfind('.') {
            Some(i) => (&n[..i], &n[i + 1..]),
            None => (n, ""),
        },
    };
    let upper_name = HeaplessUpper::from(basename);
    let upper_ext = HeaplessUpper::from(ext);
    for i in 0..8 {
        slot[i] = if i < upper_name.len {
            upper_name.bytes[i]
        } else {
            b' '
        };
    }
    for i in 0..3 {
        slot[8 + i] = if i < upper_ext.len {
            upper_ext.bytes[i]
        } else {
            b' '
        };
    }
    // Special handling for `.` / `..` — FAT stores them with a literal
    // dot-name (`.` then 7 spaces, or `..` then 6 spaces).
    if name == "." || name == ".." {
        for b in slot.iter_mut().take(11) {
            *b = b' ';
        }
        slot[0] = b'.';
        if name == ".." {
            slot[1] = b'.';
        }
    }
    slot[11] = attrs;
    // bytes 12..20: NT res + creation time/date; leave zero.
    let cluster_lo = (cluster & 0xFFFF) as u16;
    let cluster_hi = ((cluster >> 16) & 0xFFFF) as u16;
    slot[20..22].copy_from_slice(&cluster_hi.to_le_bytes());
    slot[26..28].copy_from_slice(&cluster_lo.to_le_bytes());
    slot[28..32].copy_from_slice(&size.to_le_bytes());
}

/// Tiny stack-allocated 11-byte buffer for upper-cased name/ext fields. Avoids
/// heap traffic on every directory write.
struct HeaplessUpper {
    bytes: [u8; 11],
    len: usize,
}

impl HeaplessUpper {
    fn from(s: &str) -> Self {
        let mut bytes = [0u8; 11];
        let mut len = 0;
        for (i, &b) in s.as_bytes().iter().enumerate().take(11) {
            bytes[i] = b.to_ascii_uppercase();
            len = i + 1;
        }
        Self { bytes, len }
    }
}

/// Decode the 8.3 short name from a directory entry. We always lowercase the
/// result: FAT names are case-insensitive in lookup (so `eq_ignore_ascii_case`
/// would match either way) but the shell prints the listing verbatim, and our
/// seed tree is all-lowercase ASCII. The NT-reserved flags would let us honour
/// per-file case, but the `fatfs` crate sets them inconsistently — easier to
/// just normalise to lowercase here.
fn decode_short_name(entry: &[u8]) -> String {
    let raw_name = &entry[0..8];
    let raw_ext = &entry[8..11];

    let mut name = String::new();
    for &b in raw_name.iter() {
        if b == b' ' {
            break;
        }
        name.push(b as char);
    }
    let mut ext = String::new();
    for &b in raw_ext.iter() {
        if b == b' ' {
            break;
        }
        ext.push(b as char);
    }
    if !ext.is_empty() {
        name.push('.');
        name.push_str(&ext);
    }
    name.make_ascii_lowercase();
    name
}
