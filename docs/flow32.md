# flow32 — M32: architecture & runtime flows (Developer tooling)

> Derived from `docs/idea.md` §17 and the work32 plan.

## Component map

```
        host (developer)
        +-------------+
        |  gdb        |<--- RSP over serial/virtio-console ---+
        +-------------+                                       |
                                                              v
        kernel: debug::gdb stub (halt_all on bp)
        kernel: debug::coredump (oops + userspace SIGSEGV)
                       │
                       v
                /var/crash/<id>.mdmp
                       │
                       v userspace
        +---------------------+      +---------------------+
        |  willo-symbolicate  |<-----| /usr/lib/debug/...  |
        +----------+----------+      +---------------------+
                   |
                   v
        +---------------------+
        |  willoc-crash UI    |
        +---------------------+

        userspace
        +-----------+   uprobe (M29)   +-----------+
        |willo-     |<---------------> |  syscall  |
        |strace     |                  |  entry/   |
        +-----------+                  |  exit     |
                                       +-----------+
```

## GDB stub session

```
QEMU started with -serial tcp::1234,server
gdb> target remote :1234
  -> stub `qSupported` exchange
  -> gdb requests `g` (read regs)
       stub returns RAX..R15, RIP, RFLAGS, ...
  -> gdb sets breakpoint Z0 at 0xff..addr
       stub patches INT3
  -> gdb continues `c`
  -> CPU hits INT3
  -> #BP handler:
       halt_all_cpus (IPIs)
       send STOP packet to gdb
  -> gdb steps `s`
       stub sets TF, resumes; #DB on next instr; STOP
  -> gdb detaches
       stub removes breakpoints; resumes all CPUs
```

## willo-strace flow (M29-backed)

```
willo-strace ls
  -> spawn `ls`; capture pid
  -> attach uprobe at libc::__syscall and at all kernel syscall entries
       (eBPF prog logs (pid, num, args, ret) into ringbuf)
  -> userland reader prints decoded line-by-line
exit -> detach uprobes; show summary
```

## Coredump (userspace SIGSEGV)

```
process X dereferences nullptr -> #PF -> kernel page fault handler
  pid X aux=Native; not handled -> deliver SIGSEGV
  signal handler default: dump
       -> coredump::write_minidump(X)
            collect: regs, stacks, VMA map, modules+build_ids
            format: minidump v1
            file: /var/crash/{boot_id}-{pid}-{ts}.mdmp
       -> notify §36 crash UI (D-Bus signal)
  process killed
```

## Kernel oops dump

```
kernel panic("..", file, line)
  -> save CPU state (excluding offender) via halt_all
  -> coredump::write_kernel_minidump(reason, regs, stack, modules)
       -> serial dump (QEMU) or /var/crash/kernel-<boot_id>.mdmp
  -> halt or reboot per /etc/willo/oops.toml
```

## Symbolicator

```
willo-symbolicate /var/crash/X.mdmp
  for each module in mdmp:
     bid = build_id
     debug = /usr/lib/debug/.build-id/{bid[0:2]}/{bid[2:]}.debug
     load DWARF
  for each frame in each thread:
     resolve (module + offset) -> (file, line, function)
  emit JSON + pretty text
```

## `willo-doc` flow

```
willo-doc serve --root docs/
  -> walk docs/, build index (titles, headers, links)
  -> render Markdown -> HTML (Tera-class templates)
  -> serve over HTTP on :3000
  -> live reload: inotify on docs/ → rebuild changed files
```

## Failure paths

- **GDB connection drop mid-debug** → stub continues halted; another connect re-attaches; safe.
- **Breakpoint patch on read-only text** (M22 W^X) → stub returns `E.30`; gdb shows error.
- **Minidump partial** (disk full) → header marks "incomplete"; symbolicator handles gracefully.
- **DWARF mismatch** (wrong build-id) → frame shows `<no symbol>`; warn user.
- **uprobe fails to attach** (kernel built without §29 attach point) → strace falls back to ptrace-class slow path; informs user.

## Data structures

```rust
pub struct Minidump {
    pub header: MdmpHeader,              // magic "WMDM", version, ts, boot_id
    pub modules: Vec<ModuleEntry>,
    pub threads: Vec<ThreadEntry>,
    pub vmas: Vec<VmaEntry>,
    pub signal: Option<SignalInfo>,
}

pub struct ThreadEntry {
    pub tid: Tid,
    pub regs: GeneralRegs,
    pub stack_addr: u64,
    pub stack_bytes: Vec<u8>,            // capped 256 KiB
}

pub struct ModuleEntry {
    pub base: u64,
    pub size: u64,
    pub name: String,
    pub build_id: [u8; 20],
}

pub enum GdbPacket {
    QSupported(Vec<String>),
    G(Box<[u8]>),                        // write regs
    M { addr: u64, len: usize },         // read mem
    Z { kind: u8, addr: u64, len: u8 },  // set bp
    Vcont(Vec<VcontAction>),
    QXfer { object: String, annex: String, off: usize, len: usize },
}
```
