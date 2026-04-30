# flow12 — M12: architecture & runtime flows

> Derived from `docs/idea.md` §4 and the work12 plan.

## Component map

```
              +------------------------+
              |    APIC timer (10ms)   |
              +-----------+------------+
                          | IRQ 0x40
                          v
                  +---------------+
                  |   sched::tick |
                  +-------+-------+
                          |
                          v
        +--------------------------------+
        |          scheduler             |
        |  run queue | sleep queue       |
        +-----+--------+--------+--------+
              |        |        |
              v        v        v
        +--------+ +--------+ +--------+
        | Proc A | | Proc B | | Proc C |
        +---+----+ +---+----+ +---+----+
            |          |          |
            +----------+----------+
                       |
                       v
              +------------------+
              | signals  IPC     |
              | pipe usock shm   |
              +------------------+
```

## Process state machine

```
   spawn ──> Runnable <───────────┐
              │  ▲                 │
   schedule() │  │ yield/preempt   │
              v  │                 │
            Running ──blocking──> Blocked(reason)
              │
              v
            Zombie(exit) ──wait()──> reaped
```

## Preemption tick

```
APIC fires vector 0x40
  ├─ ack lapic
  ├─ current.time_left -= 1
  ├─ if current.time_left == 0:
  │     current.state = Runnable
  │     run_queue.push_back(current)
  │     next = run_queue.pop_front()
  │     ctx_switch(current, next)
  └─ iretq
```

## fork sequence

```
parent: syscall(fork)
  └─ kernel:
       ├─ child = Process::new()
       ├─ child.files = parent.files.clone()      # refcount
       ├─ child.addr_space = parent.addr_space.cow_clone()
       │     for each user PTE in parent:
       │       clear RW; mark COW; bump refcount
       │     duplicate same PTE in child
       ├─ child.regs = parent.regs;
       │  child.regs.rax = 0
       ├─ run_queue.push_back(child)
       └─ return child.pid
```

## CoW page-fault handling

```
#PF on user page where COW bit set
  ├─ refcount == 1 ?
  │    yes: clear COW, set RW, return
  │    no:  alloc new frame, copy contents,
  │         remap with RW, dec old refcount
  └─ resume user
```

## execve sequence

```
syscall(execve, path, argv, envp)
  ├─ load ELF into a *new* AddrSpace (M11 loader)
  ├─ swap process.addr_space (drop old refs)
  ├─ build new user stack with argv/envp/auxv
  ├─ regs.rip = entry; regs.rsp = stack_top
  └─ return into user (no value; new image)
```

## Signal delivery

```
on syscall/interrupt return to user:
  if pending & ~mask != 0:
     pick highest-priority signal
     if action == terminate: process.exit_with(signo)
     if action == handler:
         push sigframe onto user stack:
           saved regs, signal number, sigmask
         regs.rip = handler
         regs.rsp = sigframe
         return; user runs handler
  else:
     return normally
```

`sigreturn` syscall restores from sigframe.

## Pipe data flow

```
writer.write(buf)
  ├─ acquire pipe.lock
  ├─ while pipe.full():
  │     wait_queue.add(self); yield
  ├─ ring.push(buf)
  └─ wake_one(reader_wq)

reader.read(buf)
  ├─ acquire pipe.lock
  ├─ while pipe.empty():
  │     wait_queue.add(self); yield
  ├─ ring.pop_into(buf)
  └─ wake_one(writer_wq)
```

## Shared-memory mapping

```
shm_open("foo")
  ├─ if first opener: alloc 1 page in shm table
  └─ return fd

mmap(fd, len, MAP_SHARED)
  ├─ for offset in 0..len step PAGE:
  │     page = shm.get(fd).page(offset)
  │     addr_space.map(usr_va + offset, page, RW)
  └─ return usr_va
```

## Failure paths

- Out-of-memory in `fork` → return `-ENOMEM`; partial state torn down.
- Pipe peer closed → reader `read()` returns 0; writer `write()` gets `SIGPIPE`.
- Scheduler bug (current ptr lost) → kernel panic (intentional in M12).
