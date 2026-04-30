# work45 — M45: Loadable kernel modules + signing + workqueues

> Derived from `docs/idea.md` §4 (Kernel Core — loadable kernel modules with versioned ABI, kthread workqueues).

## Goal

Add a real loadable-module surface to Willo. Define a `.wko` (Willo Kernel Object) format with ELF + metadata; a `vermagic`-class kernel-version-hash binding; X.509-signed module verification rooted at §38's trust anchor; `willomodprobe` resolving dependencies via a `modules.dep`-class file; **EXPORT_SYMBOL** + **EXPORT_SYMBOL_GPL** namespacing; and a kernel **workqueue** (`kthread_create_worker` / `delayed_work`) for deferred kernel work.

## Depends on

- **M11** — userspace + ELF.
- **M19** — `.willo` packaging (modules ship as `.willo` packages).
- **M38** — trust root for signing.
- **M39** — audit (denied loads logged).

## Acceptance criteria

- [ ] `.wko` format spec lives at `docs/spec/wko.md`.
- [ ] `willomod load <name>` resolves deps via `/lib/willo-modules/<kver>/modules.dep` and loads in order.
- [ ] Signature verification mandatory in production builds; dev builds allow `--allow-unsigned`.
- [ ] `vermagic` mismatch → reject with clear error.
- [ ] `EXPORT_SYMBOL` and `EXPORT_SYMBOL_GPL` resolved at load; non-GPL module cannot resolve GPL exports.
- [ ] Kernel workqueue API (`workqueue_alloc`, `queue_work`, `queue_delayed_work`, `flush_workqueue`).
- [ ] Symbol namespacing: `EXPORT_SYMBOL_NS(sym, "ns")`; module must `MODULE_IMPORT_NS("ns")`.
- [ ] Unload path safe: refcount enforced; `module_exit` callable; orphaned kthreads detected.
- [ ] `kernel/tests/wko_load_unload.rs` and `kernel/tests/wq_delayed_work.rs` pass.

## Task breakdown

### T1. `.wko` format — `docs/spec/wko.md` + `kernel/src/module/format.rs`
- ELF `.ko` extended with sections: `.willo.modinfo`, `.willo.vermagic`, `.willo.sig`.
- `MODULE_LICENSE`, `MODULE_AUTHOR`, `MODULE_DESCRIPTION`, `MODULE_PARM_DESC`, `MODULE_VERSION`.

### T2. Loader — `kernel/src/module/loader.rs`
- Parse ELF; map sections; relocate; resolve symbols from kernel + already-loaded modules.
- Run `module_init`; register cleanup.

### T3. vermagic — `kernel/src/module/vermagic.rs`
- Build script computes hash of `(kernel version, config, abi keys)`.
- Loader rejects mismatch unless `force_load` (and audited).

### T4. Signature verification — `kernel/src/module/sig.rs`
- Append-only `.willo.sig` (ed25519 over rest of file).
- Trust anchor in §38; same root as §19 packages.

### T5. Symbol export + namespaces — `kernel/src/module/sym.rs`
- `EXPORT_SYMBOL` / `EXPORT_SYMBOL_GPL` macros emit symbol entries with license + namespace tag.
- Loader cross-checks license + namespace at resolve time.

### T6. modules.dep generator — `userspace/willodepmod/`
- Scans `/lib/willo-modules/<kver>/`; emits `modules.dep` (depender → list).
- Run at install time by §19 hook.

### T7. Workqueue API — `kernel/src/sched/workqueue.rs`
- Per-CPU + unbound worker pools; bounded queue length.
- `delayed_work` uses §timer infrastructure; `flush_workqueue`, `cancel_work`.

### T8. willomod CLI + service — `userspace/willomod/`
- `willomod load|unload|list|info|deps` mirrors `modprobe`/`lsmod`.
- Auto-load by hotplug (§7 udev-class events) deferred to §48.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/module/*` | **new** module |
| `kernel/src/sched/workqueue.rs` | **new** |
| `kernel/src/syscall.rs` | + `init_module`, `delete_module`, `finit_module` |
| `userspace/willomod/` | **new** |
| `userspace/willodepmod/` | **new** |
| `docs/spec/wko.md` | **new** |

## Tests to add

- `kernel/tests/wko_load_unload.rs` — load + unload round-trip; refcount.
- `kernel/tests/vermagic_reject.rs` — mismatched vermagic rejected.
- `kernel/tests/sym_gpl_isolation.rs` — non-GPL module cannot resolve GPL export.
- `kernel/tests/wq_delayed_work.rs` — delayed_work fires near deadline.
- `userspace/willodepmod/tests/dep_topo.rs` — generated modules.dep is topologically sound.

## Risks & open questions

- **vermagic brittleness** — every kernel patch invalidates external modules; document; consider partial vermagic for ABI-stable subsystems.
- **GPL boundary policy** — Willo is project-wide MIT-friendly; `EXPORT_SYMBOL_GPL` is opt-in only when symbol must remain Linux-compatible; document case-by-case.
- **Refcount leak** — kthreads spawned by module must be tracked; `kthread_stop` mandatory on unload.
- **Live patching** — out of scope v1; future M45.x.
- **Signing UX** — vendor + Willo build keys both trusted; document key rotation in §38.
- **Out-of-tree modules** — supported but vermagic-pinned; users must `willodepmod` after kernel update.
