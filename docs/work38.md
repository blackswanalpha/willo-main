# work38 — M38: Full-disk encryption + secrets (LUKS2 + Argon2id, TPM 2.0 unsealing, keyring + SecretService)

> Derived from `docs/idea.md` §12 (Security & Privilege).

## Goal

Encrypt user data at rest. Implement a **LUKS2-class** on-disk format with **Argon2id** KDF, multiple key slots, and a kernel **AES-XTS** block layer between FS and §13 AHCI/NVMe. Add a **TPM 2.0** driver that seals the master key to PCR values (clevis-class), so boot is automatic on a clean machine and refuses to unlock if firmware/secure-boot state changed. Ship a kernel **keyring** + a userland **SecretService** for app secrets. Tie §19 signed-package verification to the same trust root.

## Depends on

- **M13** — block devices (AHCI/NVMe).
- **M16** — WilloFS (lives on top of crypt layer).
- **M37** — multi-user (per-user secrets).
- **M19** — package verification trust root.

## Acceptance criteria

- [ ] LUKS2 header read+write; multiple key slots; Argon2id KDF.
- [ ] AES-XTS-512 block layer encrypts/decrypts in 4 KiB sectors at line rate (>500 MB/s on AES-NI).
- [ ] TPM 2.0 driver works in QEMU swtpm; PCR sealing for master key (clevis-luks-bind-class).
- [ ] Boot flow: bootloader unseals via TPM at PCR 0+7; kernel loads; root mounted; user `/home` mounted on login.
- [ ] If PCR mismatch (firmware bumped), prompts for passphrase.
- [ ] Kernel keyring exposes `add_key`/`request_key`/`keyctl`-class syscalls.
- [ ] SecretService daemon exposes the freedesktop.org spec API on D-Bus; per-app keys isolated.
- [ ] §19 package verification consumes the same trust root.
- [ ] `kernel/tests/luks2_open.rs` and `kernel/tests/tpm_unseal.rs` pass.

## Task breakdown

### T1. Crypt block layer — `kernel/src/crypt/`
- `CryptBlock` device: wraps an underlying `BlockDevice`; transforms each sector AES-XTS.
- AES-NI fast path; pure-Rust fallback.
- Plumbed in §13 device tree above AHCI/NVMe and below WilloFS.

### T2. LUKS2 reader+writer — `kernel/src/crypt/luks2.rs`
- Header parse (binary + JSON metadata), key slot decode, AF-splitter, master-key recovery.
- Open path: derive slot key from passphrase via Argon2id; AES-CBC-decrypt slot to recover MK; init AES-XTS context.

### T3. Argon2id — `kernel/src/crypto/argon2.rs`
- RFC 9106; parallelism via per-CPU pools.
- Pin params at create time; benchmark to target ≥1 s on the install machine.

### T4. TPM 2.0 driver — `kernel/src/tpm/`
- TIS interface for x86; CRB for ARM (deferred).
- Commands: `Startup`, `PCR_Extend`, `PCR_Read`, `Create`, `Load`, `Unseal`, `PolicyPCR`.

### T5. clevis-class binding tool — `userspace/willocrypt/`
- `willocrypt luks bind /dev/sda1 tpm` — generates random secret, seals to PCR 0,7; embeds in LUKS metadata as a "tpm" token.
- Boot-time hook reads token and unseals before passphrase prompt.

### T6. Keyring kernel subsystem — `kernel/src/keyring/`
- Per-process and session keyrings; types: user, logon, asymmetric, encrypted.
- Syscalls: `add_key`, `request_key`, `keyctl`.

### T7. SecretService daemon — `userspace/willoc-secrets/`
- D-Bus `org.freedesktop.secrets`.
- Per-collection master key (default keyring), unlocked by user password at login.

### T8. Trust root unification — `userspace/willo-pkg/trust.rs`, `kernel/src/firmware/willo_boot.rs`
- §19 package signature root + §38 firmware signing root + §3 secure-boot key share a `/etc/willo/trust/` layout.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/crypt/*` | **new** module |
| `kernel/src/crypto/argon2.rs` | **new** |
| `kernel/src/tpm/*` | **new** module |
| `kernel/src/keyring/*` | **new** module |
| `userspace/willocrypt/` | **new** |
| `userspace/willoc-secrets/` | **new** |
| `userspace/willo-pkg/trust.rs` | extend |
| `bootloader/` | TPM unseal hook |

## Tests to add

- `kernel/tests/luks2_open.rs` — open + read + write a LUKS2 image.
- `kernel/tests/aesxts_perf.rs` — throughput target on AES-NI.
- `kernel/tests/argon2id_rfc9106.rs` — vectors.
- `kernel/tests/tpm_unseal.rs` — bind + unbind + unseal in swtpm.
- `userspace/willoc-secrets/tests/secrets_api.rs` — D-Bus API conformance.

## Risks & open questions

- **PCR brittleness** — every firmware update breaks unseal; document `willocrypt regen-binding` UX + post-update warning.
- **Recovery passphrase loss** — design forces a recovery-slot generation at install; UX prompts to print it.
- **TPM cloning attack** — TPM is bound to the device; document threat model.
- **AES-NI absence** — pure-Rust path is slow; warn user on install if no AES-NI.
- **Key slot count** — LUKS2 has 32; cap willocrypt UX to 8 to keep manageable.
- **Header backup** — `willocrypt header backup` mandatory before any operation that mutates header.
