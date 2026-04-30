# work33 — M33: Self-hosted Rust toolchain (rustc bootstrap stages, target.json, lld port)

> Derived from `docs/idea.md` §17 (Developer Tooling — long-term self-hosting).

## Goal

Build the Willo toolchain on Willo. Define a Willo target triple (`x86_64-unknown-willo`), register it in rustc's bootstrap, port **lld** for ELF linking on Willo, build `core`/`alloc`/`std` for Willo, then bootstrap **stage1 rustc** running on Willo using a Linux **stage0** snapshot. Build cargo on Willo. Reach a reproducible-build pipeline where `stage1 → stage2` is byte-identical.

## Depends on

- **M11–M19** — fully working userspace + libc-shim (M24).
- **M22** — virtio-gpu (compositor for `cargo` log UX).
- **M37** — multi-user (toolchain installs per-user or system-wide).

## Acceptance criteria

- [ ] `x86_64-unknown-willo.json` target spec lives in `tools/target/`; consumed by both Linux cross-compile and on-Willo build.
- [ ] Snapshot rustc on Linux can cross-compile `core`/`alloc`/`std` for Willo.
- [ ] Stage1 rustc binary runs on Willo and compiles a "hello, world".
- [ ] Stage1 rustc on Willo builds stage2 rustc; `diff stage1 stage2` is byte-identical.
- [ ] `cargo` builds on Willo and produces working binaries.
- [ ] `lld` ELF mode used; produces ELF binaries the M11 loader accepts.
- [ ] `kernel/tests/toolchain_hello_world.rs` (userspace integration) compiles and runs hello-world end-to-end on Willo.
- [ ] Reproducible-build pipeline documented in `docs/spec/repro.md`.

## Task breakdown

### T1. Target spec — `tools/target/x86_64-unknown-willo.json`
- Endianness little, pointer 64, alignment LP64, calling conv System V, linker `lld`, link-args = (Willo's libc-shim layout), pre-link-args, post-link-args.

### T2. rustc bootstrap registration — vendor/`rustc`
- Add `x86_64-unknown-willo` to `STAGE0_MISSING_TARGETS`.
- Patch `rustc_target/src/spec/` for the new target.
- Document the Linux-stage0 build path.

### T3. lld port — `tools/lld-willo/`
- Build LLVM `lld` (ELF mode) for Willo target.
- Linker scripts compatible with M11 ELF loader.

### T4. `core`/`alloc`/`std` — `tools/std-willo/`
- Cross-compile `core` and `alloc` first.
- Implement `std` `sys/willo/` shim using libc-shim (M24) for I/O, threads, time, network.

### T5. Stage1 build pipeline — `tools/bootstrap/`
- Driver scripts using xtask-style approach.
- Stage progression: snapshot (Linux) → stage1 (cross) → stage2 (on Willo).

### T6. cargo on Willo — `tools/cargo-willo/`
- Vendored cargo at pinned commit; depends on `std-willo` for I/O.

### T7. Reproducible-build pipeline — `tools/repro/`
- Pin SOURCE_DATE_EPOCH; deterministic ordering; identical output.
- CI verifies `diff stage1 stage2` byte-identical.

### T8. `rustup`-class installer — `userspace/willorup/`
- Manage toolchain channels (stable/beta/nightly) via §19 `.willo` packages.

## New / modified files

| Path | Change |
| --- | --- |
| `tools/target/x86_64-unknown-willo.json` | **new** |
| `tools/lld-willo/` | **new** (LLVM lld vendored) |
| `tools/std-willo/` | **new** (Willo `sys` shim) |
| `tools/bootstrap/` | **new** xtask drivers |
| `tools/cargo-willo/` | **new** |
| `tools/repro/` | **new** repro scripts |
| `userspace/willorup/` | **new** |
| `docs/spec/repro.md` | **new** |

## Tests to add

- `tools/bootstrap/tests/stage1_hello.rs` — stage1 builds + runs a hello-world.
- `tools/bootstrap/tests/stage_diff.rs` — stage1 == stage2 byte-identical.
- `kernel/tests/toolchain_hello_world.rs` — full pipeline on Willo.
- `tools/cargo-willo/tests/cargo_build.rs` — `cargo build` runs cleanly for a sample crate.

## Risks & open questions

- **Snapshot lag** — bleeding-edge target features land 1 month behind; pin a known-good rustc snapshot per Willo release.
- **lld build complexity** — depends on LLVM headers; vendor a pinned LLVM version.
- **`std::sys::willo`** — every libc surface gap forces a `std` patch; coordinate with §11 syscall additions.
- **Disk/RAM cost** — full toolchain on Willo is GBs; default install excludes; ship as `willorup` opt-in.
- **Reproducibility** — identical output requires deterministic file-system ordering (M16 must respect creation order or sort).
- **rustup parity** — Willo channels are not Rust upstream channels; document the divergence.
