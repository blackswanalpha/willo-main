# flow45 — M45: architecture & runtime flows (Loadable kernel modules)

> Derived from `docs/idea.md` §4 and the work45 plan.

## Component map

```
        userspace
        +-------------+   +-----------------+
        | willomod    |   | willodepmod     |
        +------+------+   +--------+--------+
               |  finit_module      |
               v                    v
        +--------------------------------+
        |   kernel module subsystem      |
        |   loader | vermagic | sig | sym|
        +-----+--------------+-----------+
              |              |
              v              v
        symbol table     workqueue worker pool
              |
              v
        active kernel + loaded .wko set
```

## .wko load

```
willomod load mydrv
  -> read /lib/willo-modules/<kver>/modules.dep
  -> resolve deps: [base, mydrv]
  -> for m in deps in order:
       fd = open("/lib/willo-modules/<kver>/$m.wko")
       finit_module(fd, params, flags=0)
kernel:
  parse ELF
  parse .willo.modinfo (license, vermagic, namespaces)
  if vermagic != current: reject ENOEXEC
  verify .willo.sig with §38 trust root; reject if invalid (unless --allow-unsigned)
  allocate module struct; map sections
  resolve symbols against kernel + loaded modules
     license check: GPL-only target requires GPL caller
     namespace check: target requires MODULE_IMPORT_NS in caller
  apply relocations
  call module_init() -> Ok | Err
  on Err: free module; return Err
  on Ok: register module; refcount=0
```

## Symbol resolution

```
__willo_export_table holds (sym_name, addr, license_tag, namespace)
loader for unresolved symbol s:
  look in __willo_export_table -> entry e
  if e.license == GPL && module.license != GPL: reject
  if e.namespace != "" && !module.imports(e.namespace): reject
  patch reloc with addr
```

## Unload

```
willomod unload mydrv
  -> kernel module->refcount must be 0
  -> module_exit() called
  -> kthreads owned by module: detect via kthread tags; warn if non-zero
  -> deregister exports
  -> free pages
returns -EBUSY if any user (e.g. open file with this driver active)
```

## Workqueue

```
let wq = workqueue_alloc("mywq", WQ_UNBOUND | WQ_HIGHPRI, max_active=4);
let work = work_init(my_handler, my_data);
queue_work(wq, work);     // runs ASAP on a worker thread
queue_delayed_work(wq, work, 5_000);   // 5s later via timer
flush_workqueue(wq);     // wait for all queued items
destroy_workqueue(wq);
internal:
  per-CPU pool of kworker kthreads
  WQ_UNBOUND uses a global pool
  max_active prevents kthread storm
```

## modules.dep generation

```
willodepmod /lib/willo-modules/<kver>/
  for each .wko:
    parse .willo.modinfo
    record exports + imports
  topological sort by import dependency
  write modules.dep:
    mydrv: base
    base:
```

## Failure paths

- **Vermagic mismatch** → load denied; user prompted to rebuild module; §39 audit log.
- **Bad signature** → load denied; never logged with key bytes; surfaced as "module signature invalid".
- **Symbol unresolved** → load denied; loader names the missing symbol.
- **module_init returns Err** → state cleaned; partial side effects (kthreads, devices) responsibility of module to reverse before returning Err.
- **Unload while busy** → -EBUSY; surface refcount info.
- **kthread leak on unload** → detected; load denied next time until clean.

## Data structures

```rust
pub struct WkoModule {
    pub name: SmolStr,
    pub vermagic_hash: [u8; 32],
    pub license: License,                    // Mit | Bsd | Apache2 | Gpl | Proprietary
    pub author: Option<SmolStr>,
    pub description: Option<SmolStr>,
    pub version: SmolStr,
    pub imports_ns: Vec<SmolStr>,
    pub exports: Vec<ExportEntry>,
    pub init: extern "C" fn() -> i32,
    pub exit: extern "C" fn(),
    pub refcount: AtomicI32,
    pub state: ModuleState,                  // Live | Going | Coming
}

pub struct ExportEntry {
    pub sym: SmolStr,
    pub addr: VirtAddr,
    pub license_tag: License,                // GPL | Any
    pub namespace: SmolStr,                  // "" = no namespace
}

pub struct WorkQueue {
    pub name: SmolStr,
    pub flags: WqFlags,                      // Unbound | HighPri | CpuIntensive
    pub max_active: u16,
    pub pool: WorkerPool,
}

pub struct DelayedWork {
    pub work: Work,
    pub deadline: TickInstant,
    pub timer: TimerHandle,
}
```
