# flow47 — M47: architecture & runtime flows (A/B slots + measured boot)

> Derived from `docs/idea.md` §3 and the work47 plan.

## Component map

```
        UEFI firmware
          |  PCR 0 (firmware) extended
          v
        bootloader
          |  AVB verify
          |  pick slot (a/b/recovery/firmware-update)
          |  extend PCR 2/4/7
          v
        kernel image (slot N)
          |  extend PCR 8 (cmdline)
          v
        initrd
          |  extend PCR 9
          v
        kernel root pivot -> systemd-class init
          |  extend PCR 10 (modules), 11 (root FS measurement)
          v
        user session (login, compositor, apps)
          |  willobootctl mark-good (§19 update daemon)
          v
        slot.successful = 1
```

## Slot metadata layout

```
bytes:  magic "WBOT" | version u8 | crc32 u32 | reserved u8
slot_a: { active: bool, successful: bool, retry: u8, priority: u8 }
slot_b: same
boot_next: u8 (a|b|recovery|firmware)
default:   u8
```

Stored:
- EFI: variable `WilloSlotMeta` (NV+RT+BS).
- BIOS: LBA 33 of disk (single sector; atomic).

## AVB verify flow

```
bootloader:
  read vbmeta partition
  verify ed25519 signature against trust root
  for descriptor in vbmeta.descriptors:
     if HASH: read partition; verify sha256 matches
     if HASHTREE: read tree root; later kernel uses dm-verity-class
     if CHAINED: recurse into chained vbmeta
  enforce rollback:
     for n in 0..ROLLBACK_SLOTS:
        if vbmeta.rollback_index[n] < stored_rollback_index[n]: ABORT
  on ok: extend PCR 4 with vbmeta hash
```

## Boot menu

```
+-----------------------------------+
|         willo boot menu           |
| > Willo (slot A)  [default]       |
|   Willo (slot B)                  |
|   Recovery                        |
|   Firmware update                 |
+-----------------------------------+
  arrow keys; Enter to boot
  timeout = 5s -> default selection
```

## Update install flow

```
willo-updd applies update:
  -> stage to inactive slot (e.g., B)
  -> verify integrity offline (avb sign)
  -> set slot_meta:
       slot_b.priority = 2 (higher)
       slot_b.retry = 3
       slot_b.successful = 0
       boot_next = B
  -> reboot
```

## Successful first boot

```
boot picks slot B (boot_next)
  retry-- in slot_b (now 2)
  kernel boots
  user logs in
  willologin / willocomp running 30s
  willo-updd watches uptime + journal:
     no kernel oops
     compositor up
     network up (basic HTTP)
  -> willobootctl mark-good
       slot_b.successful = 1
       slot_b.retry = 0 (don't decrement further)
```

## Rollback after 3 failed boots

```
boot 1: retry 3 -> 2; oops mid-boot
boot 2: retry 2 -> 1; oops
boot 3: retry 1 -> 0; oops
boot 4: bootloader sees retry=0 && successful=0
  -> swap active to A
  -> reset slot_b retry, mark "failed"
  -> boot A
journal records the rollback event
```

## Measured boot summary

```
PCR 0  = sha(firmware)                       [UEFI]
PCR 2  = sha(extended firmware code)         [UEFI]
PCR 4  = sha(bootloader + vbmeta header)     [bootloader]
PCR 7  = sha(secure boot state + db keys)    [UEFI]
PCR 8  = sha(kernel cmdline)                 [bootloader pre-jump]
PCR 9  = sha(initrd)                         [bootloader pre-jump]
PCR 10 = sha(loaded module set)              [kernel]
PCR 11 = sha(root FS root hash via verity)   [kernel]
```

## §38 LUKS unseal interaction

```
seal policy: PCR 0,2,4,7 (boot chain integrity)
on update applied:
  PCR 4 changes (new bootloader)
  -> next boot: TPM unseal fails
  -> fall back to passphrase
  -> after first successful login, willocrypt rebind: re-seal to current PCR set
  -> subsequent boots auto-unlock
```

## Failure paths

- **Bad signature on vbmeta** → bootloader hangs at "boot integrity failure"; offers Recovery.
- **PCR mismatch with sealing** → §38 falls back to passphrase; user warned + post-update rebind.
- **Rollback while in Recovery** → recovery doesn't decrement retry; slot machinery untouched.
- **EFI write fails** (NV exhaustion) → fall back to BIOS LBA 33; warn at every boot until cleared.
- **Both slots fail** → bootloader stops after rollback; user must boot Recovery and reinstall.

## Data structures

```rust
#[repr(C)]
pub struct SlotMeta {
    pub magic: [u8; 4],                      // b"WBOT"
    pub version: u8,
    pub crc32: u32,
    pub reserved: u8,
    pub slots: [SlotInfo; 2],                // [a, b]
    pub boot_next: u8,                       // 0=a, 1=b, 2=recovery, 3=fw-upd
    pub default_slot: u8,
}

#[repr(C)]
pub struct SlotInfo {
    pub active: u8,
    pub successful: u8,
    pub retry: u8,
    pub priority: u8,
}

pub struct AvbVbMeta {
    pub magic: [u8; 4],                      // b"AVB0"
    pub algorithm: SignAlg,                  // Ed25519 | Rsa4096Sha512
    pub auth_block: Vec<u8>,
    pub aux_block: Vec<u8>,                  // descriptors live here
    pub rollback_index: [u64; 8],
}

pub enum AvbDescriptor {
    Hash { partition: SmolStr, hash: [u8; 32], image_size: u64 },
    Hashtree { partition: SmolStr, root_digest: [u8; 32], block_size: u32, height: u8 },
    Chained { partition: SmolStr, public_key: Vec<u8>, rollback_index_location: u32 },
}
```
