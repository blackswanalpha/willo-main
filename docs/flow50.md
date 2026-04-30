# flow50 — M50: architecture & runtime flows (Native IDE / DAP + LSP)

> Derived from `docs/idea.md` §17 and the work50 plan.

## Component map

```
                       +-----------------+
                       |    willedit     |
                       |  (editor core)  |
                       +--+----+----+----+
                          |    |    |
              dap         lsp  syntax  workspace_search
              client      pool ts      (willo-rg)
              |           |    |       |
              v           v    v       v
        willodap-{rust,c,py} per-lang  ripgrep-class
              |              servers   index
              v
        gdb stub (§32) /
        lldb adapter (userland)
```

## DAP request flow (set breakpoint + run)

```
willedit:
  send {seq:1, type:request, command:initialize, args}
  recv {response, capabilities}
  send launch { program, args, cwd }
  recv launched
  send setBreakpoints { source: file, lines: [42] }
  recv response { breakpoints: [{verified:true}] }
  send configurationDone
  recv stopped { reason:"entry", threadId:1 }
  send continue { threadId:1 }
  recv stopped { reason:"breakpoint", threadId:1 }
  send threads, stackTrace, scopes, variables (chained)
  user steps over -> next request, etc.
on terminate:
  send disconnect; adapter exits
```

## willodap-rust internals

```
launch program X:
  spawn lldb-class debugger backend
  load target X with DWARF
  set initial breakpoints
  run
adapter loop:
  on debugger event (stopped, exited, output):
    translate to DAP event; forward to client
  on DAP request:
    map to debugger command (b, c, n, s, finish, p)
    forward response
```

## LSP multi-root workspace

```
willedit opens [root_a, root_b]:
  start rust-analyzer once with workspaceFolders=[a, b]
  didOpen for each open file
  on edit -> didChange (incremental; tree-sitter + LSP rope sync)
  diagnostics arrive as publishDiagnostics
  jump-to-def in a -> goes through rust-analyzer; result may be in b
  workspaceSymbols query covers both roots
LSP server crashes:
  health monitor restarts with backoff (1s,2s,5s,15s)
  diagnostics pane shows "language server restarting"
```

## Tree-sitter incremental parse

```
edit at byte offset 1234 (insert 5 bytes):
  tree.edit(start_byte=1234, old_end=1234, new_end=1239,
            start_position, old_end_position, new_end_position)
  parser.parse(new_text, &tree) -> new_tree (reuses unchanged subtrees)
highlight query runs over changed_ranges only
```

## Workspace search (willo-rg)

```
willedit search "fn main"
  -> spawn willo-rg -n -i "fn main" /workspace
  -> walks directory parallel; respects .gitignore
  -> regex match per file via SIMD-accelerated regex
  -> stream results: file:line:col:match
  -> editor renders incrementally (don't block on whole walk)
```

## Git operations

```
willedit-git stage README.md:
  spawn git2 op in sandboxed worker
  apply index changes; emit "indexChanged" event
willedit-git commit "msg":
  read user.name + user.email from §38 SecretService (or git config)
  commit with parents = HEAD
  emit "headChanged"
push:
  resolve credentials via §38; auth via SSH key from keyring
  push to remote
```

## Build task run

```
.willedit/tasks.toml:
  [[tasks]]
  name = "cargo build"
  cmd = "cargo build"
  problemMatcher = "rust"
willedit run task:
  spawn task in §34 PTY
  output -> ANSI parser -> output panel
  problemMatcher parses errors -> diagnostics
  on exit: emit "taskComplete" status code
```

## Failure paths

- **DAP server crash** → editor shows "debug session ended unexpectedly"; offers restart.
- **LSP timeout** → request cancelled; UI not blocked; spinner visible.
- **Tree-sitter parse fatal** → fall back to lexer-only highlighting; warn once.
- **Search exhausts results > 10k** → cap; prompt to refine.
- **Git auth fail** → §38 prompt; never log token bytes.
- **Build task never exits** → user can cancel; SIGTERM, then SIGKILL after 5 s.

## Data structures

```rust
pub struct DapClient {
    pub seq: AtomicU64,
    pub stream: Stream,
    pub pending: HashMap<u64, oneshot::Sender<DapResponse>>,
    pub capabilities: DapCapabilities,
}

pub enum DapEvent {
    Stopped { thread_id: u64, reason: StopReason, hit_breakpoint_ids: Vec<u64> },
    Continued { thread_id: u64, all_threads: bool },
    Output { category: OutputCategory, output: String },
    Exited { exit_code: i32 },
    Terminated,
    Thread { reason: ThreadEventReason, thread_id: u64 },
}

pub struct LspServerHandle {
    pub language: SmolStr,
    pub workspace_folders: Vec<PathBuf>,
    pub child: ChildHandle,
    pub capabilities: LspCapabilities,
    pub health: ServerHealth,
}

pub struct WorkspaceSearchHit {
    pub file: PathBuf,
    pub line: u32,
    pub col: u32,
    pub matched_text: SmolStr,
    pub context_before: SmolStr,
    pub context_after: SmolStr,
}

pub struct BuildTask {
    pub name: SmolStr,
    pub cmd: SmolStr,
    pub args: Vec<SmolStr>,
    pub cwd: PathBuf,
    pub env: BTreeMap<SmolStr, SmolStr>,
    pub problem_matcher: Option<SmolStr>,
}
```
