//! ATA PIO driver (28-bit LBA).
//!
//! Polled, synchronous, no DMA. Suitable for legacy IDE attachments where
//! QEMU/VBox emulate a primary IDE controller at I/O ports `0x1F0..=0x1F7`
//! and `0x3F6`. Two devices share that bus (master + slave).
//!
//! M9 only uses the **primary slave** (the data disk), so the driver is hard-
//! wired to bus 0, drive 1. Boot disk lives on bus 0 drive 0 and is read by
//! the BIOS bootloader, not by us.
//!
//! References: ATA/ATAPI-6 spec; OSDev wiki `https://wiki.osdev.org/ATA_PIO_Mode`.

use crate::block::{BLOCK_SIZE, BlockDevice, BlockError};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::{Lazy, Mutex};
use x86_64::instructions::port::{Port, PortReadOnly, PortWriteOnly};

const PRIMARY_BASE: u16 = 0x1F0;
const PRIMARY_CTRL: u16 = 0x3F6;

const REG_DATA: u16 = 0;
const REG_SECTOR_COUNT: u16 = 2;
const REG_LBA_LO: u16 = 3;
const REG_LBA_MID: u16 = 4;
const REG_LBA_HI: u16 = 5;
const REG_DRIVE_HEAD: u16 = 6;
const REG_STATUS: u16 = 7;
const REG_COMMAND: u16 = 7;
#[allow(dead_code)]
const REG_ERROR: u16 = 1;

const STATUS_ERR: u8 = 0x01;
const STATUS_DRQ: u8 = 0x08;
const STATUS_DF: u8 = 0x20;
const STATUS_BSY: u8 = 0x80;

const CMD_READ_SECTORS: u8 = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_IDENTIFY: u8 = 0xEC;

const POLL_LIMIT: usize = 100_000;

/// Primary IDE slave (drive 1). One disk only — that's all M9 needs.
pub struct AtaDrive {
    data: Port<u16>,
    sector_count: Port<u8>,
    lba_lo: Port<u8>,
    lba_mid: Port<u8>,
    lba_hi: Port<u8>,
    drive_head: Port<u8>,
    status: PortReadOnly<u8>,
    command: PortWriteOnly<u8>,
    /// Device control register at 0x3F6 — used to set the nIEN bit so the
    /// IDE controller does NOT assert IRQ14. We poll instead.
    control: PortWriteOnly<u8>,
    sectors: u32,
}

impl AtaDrive {
    /// Create a handle to the primary slave. Does NOT touch hardware yet.
    const fn new_primary_slave() -> Self {
        Self {
            data: Port::new(PRIMARY_BASE + REG_DATA),
            sector_count: Port::new(PRIMARY_BASE + REG_SECTOR_COUNT),
            lba_lo: Port::new(PRIMARY_BASE + REG_LBA_LO),
            lba_mid: Port::new(PRIMARY_BASE + REG_LBA_MID),
            lba_hi: Port::new(PRIMARY_BASE + REG_LBA_HI),
            drive_head: Port::new(PRIMARY_BASE + REG_DRIVE_HEAD),
            status: PortReadOnly::new(PRIMARY_BASE + REG_STATUS),
            command: PortWriteOnly::new(PRIMARY_BASE + REG_COMMAND),
            control: PortWriteOnly::new(PRIMARY_CTRL),
            sectors: 0,
        }
    }

    /// IDENTIFY DEVICE on the primary slave. Fills `self.sectors` with the
    /// 28-bit-LBA sector count from the IDENTIFY response.
    pub fn identify(&mut self) -> Result<(), BlockError> {
        x86_64::instructions::interrupts::without_interrupts(|| self.identify_inner())
    }

    fn identify_inner(&mut self) -> Result<(), BlockError> {
        unsafe {
            // nIEN = mask IRQ14 from the controller. We poll instead — the
            // bootloader's IDT does not handle IDE IRQs and any unmasked
            // IRQ14 leads straight to a #GP → double fault on `sti`.
            self.control.write(0x02);
            self.drive_head.write(0xB0);
            self.delay_400ns();
            self.sector_count.write(0);
            self.lba_lo.write(0);
            self.lba_mid.write(0);
            self.lba_hi.write(0);
            self.command.write(CMD_IDENTIFY);
            self.delay_400ns();

            let s = self.status.read();
            if s == 0 {
                return Err(BlockError::DriveError);
            }
            self.wait_not_busy()?;
            if self.lba_mid.read() != 0 || self.lba_hi.read() != 0 {
                return Err(BlockError::DriveError);
            }
            self.wait_drq()?;

            let mut sectors_lo: u16 = 0;
            let mut sectors_hi: u16 = 0;
            for i in 0..256 {
                let w = self.data.read();
                if i == 60 {
                    sectors_lo = w;
                } else if i == 61 {
                    sectors_hi = w;
                }
            }
            self.sectors = (sectors_lo as u32) | ((sectors_hi as u32) << 16);
            Ok(())
        }
    }

    fn delay_400ns(&mut self) {
        // Reading the status port four times wastes ~400 ns and lets the
        // controller settle after a register write.
        for _ in 0..4 {
            unsafe {
                let _ = self.status.read();
            }
        }
    }

    fn wait_not_busy(&mut self) -> Result<(), BlockError> {
        for _ in 0..POLL_LIMIT {
            let s = unsafe { self.status.read() };
            if s & STATUS_BSY == 0 {
                if s & STATUS_ERR != 0 || s & STATUS_DF != 0 {
                    return Err(BlockError::DriveError);
                }
                return Ok(());
            }
        }
        Err(BlockError::Timeout)
    }

    fn wait_drq(&mut self) -> Result<(), BlockError> {
        for _ in 0..POLL_LIMIT {
            let s = unsafe { self.status.read() };
            if s & STATUS_BSY != 0 {
                continue;
            }
            if s & STATUS_ERR != 0 || s & STATUS_DF != 0 {
                return Err(BlockError::DriveError);
            }
            if s & STATUS_DRQ != 0 {
                return Ok(());
            }
        }
        Err(BlockError::Timeout)
    }

    fn select_lba28(&mut self, lba: u32, count: u8) {
        unsafe {
            // 0xF0: drive=1 (slave), LBA mode bit set.
            self.drive_head.write(0xF0 | (((lba >> 24) & 0x0F) as u8));
            self.delay_400ns();
            self.sector_count.write(count);
            self.lba_lo.write(lba as u8);
            self.lba_mid.write((lba >> 8) as u8);
            self.lba_hi.write((lba >> 16) as u8);
        }
    }
}

impl BlockDevice for AtaDrive {
    fn num_blocks(&self) -> u64 {
        self.sectors as u64
    }

    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<usize, BlockError> {
        if lba >= self.sectors as u64 {
            return Err(BlockError::OutOfRange);
        }
        if buf.len() < BLOCK_SIZE {
            return Err(BlockError::OutOfRange);
        }
        self.wait_not_busy()?;
        self.select_lba28(lba as u32, 1);
        unsafe {
            self.command.write(CMD_READ_SECTORS);
            self.delay_400ns();
            self.wait_drq()?;
            for chunk in buf[..BLOCK_SIZE].chunks_exact_mut(2) {
                let word = self.data.read();
                chunk[0] = word as u8;
                chunk[1] = (word >> 8) as u8;
            }
        }
        Ok(BLOCK_SIZE)
    }

    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<usize, BlockError> {
        if lba >= self.sectors as u64 {
            return Err(BlockError::OutOfRange);
        }
        if buf.len() < BLOCK_SIZE {
            return Err(BlockError::OutOfRange);
        }
        self.wait_not_busy()?;
        self.select_lba28(lba as u32, 1);
        unsafe {
            self.command.write(CMD_WRITE_SECTORS);
            self.delay_400ns();
            self.wait_drq()?;
            for chunk in buf[..BLOCK_SIZE].chunks_exact(2) {
                let word = (chunk[0] as u16) | ((chunk[1] as u16) << 8);
                self.data.write(word);
            }
            self.wait_not_busy()?;
        }
        Ok(BLOCK_SIZE)
    }
}

/// Single global drive handle. Lazy because port construction must happen at
/// runtime (Port::new is const, but we want to defer probing).
pub static DRIVE: Lazy<Mutex<AtaDrive>> = Lazy::new(|| Mutex::new(AtaDrive::new_primary_slave()));

static INITIALISED: AtomicBool = AtomicBool::new(false);

/// Probe + IDENTIFY the data disk. Idempotent. Returns `Err` if the disk is
/// missing or doesn't speak ATA — callers should treat that as "no FAT32".
pub fn init() -> Result<(), BlockError> {
    if INITIALISED.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut drive = DRIVE.lock();
    drive.identify()?;
    INITIALISED.store(true, Ordering::Release);
    Ok(())
}
