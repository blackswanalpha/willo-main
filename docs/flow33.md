# flow33 — M33: architecture & runtime flows (Self-hosted Rust toolchain)

> Derived from `docs/idea.md` §17 and the work33 plan.

## Component map

```
        Linux build host
        +------------------+         +------------------+
        | snapshot rustc   |         | LLVM/lld source  |
        +--------+---------+         +--------+---------+
                 |                            |
                 v                            v
        +------------------+         +------------------+
        | tools/bootstrap  |  -->    | tools/lld-willo  |
        | (xtask)          |         +------------------+
        +--------+---------+
                 |
                 v
        +------------------+
        | stage1 rustc     |  (binary for Willo target)
        +--------+---------+
                 |  ship via §19 .willo package
                 v
        Willo OS
        +------------------+         +------------------+
        | stage1 rustc     |  -->    | stage2 rustc     |
        | (running)        |         |                  |
        +------------------+         +--------+---------+
                                              |
                                              v
                                       diff(stage1,stage2)
                                       must be byte-identical
```

## Stage progression

```
Stage 0 (snapshot, on Linux):
  - upstream Rust prebuilt
  - patched only with Willo target spec

Stage 1 (cross-compile, on Linux, target Willo):
  - snapshot rustc + tools/std-willo
  - produces:
       libcore-willo.rlib
       liballoc-willo.rlib
       libstd-willo.rlib
       rustc binary (Willo)
       lld binary (Willo)

Stage 2 (self-host, on Willo):
  - run stage1 rustc + cargo on Willo
  - re-build rustc + std for Willo
  - output: stage2 rustc

Verification: stage1 vs stage2 diff bit-for-bit
```

## Cross-compile std

```
cargo build --target x86_64-unknown-willo -p std
  -> rustc reads target.json
  -> compiles core (no_std) -> rlib
  -> compiles alloc -> rlib
  -> compiles std with sys/willo/ glue calling libc-shim (M24)
       -> std::fs -> POSIX-shaped openat/read/write
       -> std::thread -> Willo threads (clone3-shaped)
       -> std::time -> clock_gettime
       -> std::net -> §14 socket API
  -> rlib emitted
```

## Hello-world on Willo

```
willoshell$ cat > hello.rs <<EOF
fn main() { println!("hello, willo"); }
EOF
willoshell$ rustc hello.rs -o hello
  -> rustc parses, type-checks, codegens object .o
  -> calls lld to link with libstd-willo.rlib + libc-shim
  -> writes ELF "hello"
willoshell$ ./hello
hello, willo
```

## Reproducible build verification

```
SOURCE_DATE_EPOCH=1714435200 cargo build --release ... | tee log1
mv target/release/rustc /tmp/r1
cargo clean
SOURCE_DATE_EPOCH=1714435200 cargo build --release ... | tee log2
mv target/release/rustc /tmp/r2
sha256sum /tmp/r1 /tmp/r2
  -> two identical hashes (expected)
diff -u log1 log2
  -> identical (modulo timing — filtered)
```

## willorup install flow

```
willoshell$ willorup install nightly
  -> resolve channel "nightly" → version 1.95.0-nightly-willo.42
  -> §19 willo-pkg install rust-nightly-1.95.0-nightly-willo.42.willo
  -> sets ~/.willorup/toolchains/nightly/bin in PATH
  -> ln -s nightly ~/.willorup/active
willoshell$ rustc --version
rustc 1.95.0-nightly-willo.42 (x86_64-unknown-willo)
```

## Failure paths

- **Cross-compile fails** (missing libc-shim symbol) → bootstrap log lists missing symbols; fix in M24 libc-shim then retry.
- **lld link error** (linker script mismatch) → verify pre/post-link-args in target.json; commonly an LDFLAGS oversight.
- **Stage1 rustc segfaults on Willo** → minidump captured by §32; symbolicate against rustc DWARF.
- **Stage1 != Stage2** → check timestamps embedded; rebuild with SOURCE_DATE_EPOCH; sort dir entries.
- **Disk full during stage2** → installer warns; build aborts cleanly.

## Data structures

```rust
// tools/target/x86_64-unknown-willo.json (excerpt, conceptual)
pub struct WilloTarget {
    pub triple: "x86_64-unknown-willo",
    pub arch: Arch::X86_64,
    pub data_layout: "e-m:e-i64:64-i128:128-...",
    pub linker: Linker::Lld,
    pub pre_link_args: &'static [&'static str],
    pub post_link_args: &'static [&'static str],
    pub target_pointer_width: 64,
    pub features: &'static [Feature],
    pub panic_strategy: PanicStrategy::Unwind,
}

// tools/std-willo/sys/mod.rs (sketch)
pub mod sys {
    pub mod fs;       // openat/read/write/close via libc-shim
    pub mod thread;   // clone3
    pub mod time;     // clock_gettime
    pub mod net;      // socket/bind/connect
    pub mod env;      // environ via libc-shim
    pub mod process;  // fork/exec
}
```
