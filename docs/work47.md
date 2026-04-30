# work47 — M47: A/B kernel slots + measured boot end-to-end

> Derived from `docs/idea.md` §3 (Boot & Firmware — A/B kernel slots, secure boot, TPM measured boot, boot menu).

## Goal

Make Willo updates safe. Implement Android-class **A/B slot** semantics across the **bootloader → kernel → initrd → user session** chain, signed end-to-end, **TPM measured boot** with PCRs 0..7 extended at each stage, **AVB-class** rollback indexes preventing downgrade, and a UEFI **boot menu** with kernel/recovery/firmware-update entries. Failed boot (3 attempts) auto-rolls back.

## Depends on

- **M19** — package + update daemon (writes new slot).
- **M38** — TPM driver + sealing (PCR-bound LUKS already lives here).
- **M16** — journal (boot logs).
- **M49** — recovery environment (alt boot target).

## Acceptance criteria

- [ ] Two slots `a` and `b`; either can be active; switch is atomic.
- [ ] EFI variables `LoaderEntrySelected`, `LoaderEntryDefault`, `WilloBootCount`, `WilloRollback` round-trip.
- [ ] AVB-class vbmeta partition signed end-to-end; PCR 0/2/4 extended for firmware/code/kernel; PCR 7 for secure-boot state.
- [ ] Rollback indexes prevent downgrade.
- [ ] Boot menu enumerates kernel A, kernel B, recovery, firmware-update; user can pick.
- [ ] After 3 consecutive failed boots, slot reverts.
- [ ] `kernel/tests/firmware_slot_swap.rs` and `kernel/tests/measured_boot_pcrs.rs` pass.

## Task breakdown

### T1. Slot metadata — `bootloader/slot_meta.rs`
- Layout: magic, version, crc32, two `SlotInfo { active, successful, retry, priority }`.
- Stored in EFI variables and in a known LBA on disk for BIOS path.

### T2. AVB-class verifier — `bootloader/avb.rs`
- Read vbmeta; verify signature; load hash descriptors per partition.
- Check `rollback_index[n] >= stored_rollback_index[n]`.

### T3. PCR extend chain — `bootloader/measure.rs` + `kernel/src/tpm/measure.rs`
- Bootloader extends PCR 0 (firmware), PCR 2 (extended code), PCR 4 (boot loader), PCR 7 (secure boot state).
- Kernel extends PCR 8 (kernel cmdline), PCR 9 (initrd hash), PCR 10 (modules), PCR 11 (root FS).

### T4. Boot menu — `bootloader/menu.rs`
- 5 s timeout (configurable); arrow + enter; entries: A, B, Recovery (§49), Firmware Update.
- Branding via §17 framebuffer logo (pre-kernel).

### T5. Update daemon hook — `userspace/willo-updd/slot.rs`
- Write new artefacts to inactive slot; set `boot_next = inactive`, `retry = 3`, `successful = 0`; reboot.
- On successful login + 30 s, mark new slot `successful=1`.

### T6. Rollback — `bootloader/rollback.rs`
- On boot, decrement `retry`; if `retry == 0` and `successful == 0`, swap active back; reboot.

### T7. `willobootctl` — `userspace/willobootctl/`
- `list-slots`, `set-active`, `set-default`, `mark-good`, `commit-update`.
- §39 audit each.

### T8. Boot logs end-to-end — feeds §16
- Bootloader writes a boot record to a known LBA; kernel ingests and emits journal events.

## New / modified files

| Path | Change |
| --- | --- |
| `bootloader/{slot_meta,avb,measure,menu,rollback}.rs` | **new** |
| `kernel/src/tpm/measure.rs` | **new** |
| `kernel/src/firmware/willo_boot.rs` | extend (slot + measure surfaces) |
| `userspace/willo-updd/slot.rs` | **new** |
| `userspace/willobootctl/` | **new** |
| `docs/spec/avb-willo.md` | **new** AVB profile |

## Tests to add

- `kernel/tests/firmware_slot_swap.rs` — swap + reboot; new slot active; old preserved.
- `kernel/tests/firmware_rollback_after_3_fails.rs` — fake fails; rollback engages.
- `kernel/tests/measured_boot_pcrs.rs` — PCR values match expected per artefacts.
- `userspace/willobootctl/tests/cli.rs` — CLI happy path.

## Risks & open questions

- **PCR brittleness** (interacts with §38) — every artefact change rebinds clevis; document UX flow.
- **Bootloader↔kernel ABI** — slot bytes are irreversible once shipped; freeze in `docs/spec/`.
- **Disk-only path (BIOS)** — EFI vars unavailable; use a reserved sector; write must be atomic (single 512 B sector).
- **Recovery mode entry** — must not trip rollback; recovery boots with `successful=untracked`.
- **Firmware-update slot** — separate flow; don't share the kernel A/B counter.
- **vbmeta size** — bounded; bigger partitions hashed via dm-verity-class hashtree.
