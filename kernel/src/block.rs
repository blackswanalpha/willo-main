//! Generic block-device abstraction. M9 only ships an ATA PIO driver in
//! `crate::ata`; later milestones may add VirtIO, AHCI, NVMe.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// Drive timed out responding to a status poll.
    Timeout,
    /// Drive reported the ERR bit in its status register.
    DriveError,
    /// Caller asked for an LBA past the end of the disk.
    OutOfRange,
}

pub const BLOCK_SIZE: usize = 512;

/// 512-byte-block device. Methods take `&mut self` because reads/writes mutate
/// hardware registers; in practice we own a single global driver behind a
/// `spin::Mutex`.
pub trait BlockDevice {
    /// Total number of 512-byte blocks the disk holds.
    fn num_blocks(&self) -> u64;

    /// Read a single 512-byte block at `lba` into `buf` (which must be at
    /// least 512 bytes). Returns the number of bytes written into `buf`.
    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<usize, BlockError>;

    /// Write a single 512-byte block at `lba` from `buf` (which must be at
    /// least 512 bytes). Returns the number of bytes consumed from `buf`.
    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<usize, BlockError>;
}
