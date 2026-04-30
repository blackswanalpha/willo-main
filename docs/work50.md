# work50 — M50: Native IDE / DAP + LSP suite (extending willedit)

> Derived from `docs/idea.md` §17 (Developer Tooling — native IDE long-term).

## Goal

Promote §34's `willedit` into a real IDE. Land a **Debug Adapter Protocol (DAP)** client that drives §32's GDB stub and userspace processes via a **native debug adapter** (`willodap-rust`, `willodap-c`); a **multi-language LSP** orchestration layer; **tree-sitter** incremental parsing; **ripgrep**-class workspace search; build orchestration for `cargo`, `make`, `cmake`; and a Git client (`willedit-git`).

## Depends on

- **M34** — `willedit` (extended, not replaced).
- **M32** — minidump + GDB stub (debug back-end).
- **M33** — self-hosted toolchain (`cargo` runs on Willo).
- **M37** — multi-user (per-user workspace settings).
- **M39** — sandboxing (LSP servers run sandboxed).

## Acceptance criteria

- [ ] DAP client implements: launch/attach, breakpoints (line + conditional + logpoint), threads, scopes, variables, stack frames, evaluate, step over/into/out, restart, terminate.
- [ ] `willodap-rust` debugs Rust programs; `willodap-c` debugs C/C++; `willodap-py` for Python.
- [ ] LSP orchestration: per-language server pool; warm-start; restart on crash; multi-root workspaces.
- [ ] Tree-sitter incremental parse: edits sync without full reparse; highlight stable.
- [ ] Workspace search via ripgrep-class engine: <100 ms on 100k-file repo.
- [ ] Git client: status, diff, stage, commit, push, pull, branch ops; runs out-of-process; sandboxed.
- [ ] Build orchestration: `cargo build/run/test`, `make`, `cmake --build`, custom tasks; output panel with ANSI parsing.
- [ ] `userspace/willedit/tests/dap_breakpoint.rs`, `userspace/willedit/tests/lsp_multiroot.rs`, `userspace/willedit/tests/workspace_search.rs` pass.

## Task breakdown

### T1. DAP client — `userspace/willedit/dap/`
- JSON-RPC client over stdio (and TCP).
- Implement DAP request/response/event types in Rust.
- UI: variables, watch, call stack, breakpoints panes.

### T2. willodap-rust — `userspace/willodap-rust/`
- DAP server backed by §32 GDB stub for kernel + LLDB protocol via §33's lldb-class adapter for userland Rust.
- Use rust-analyzer DWARF for symbol mapping.

### T3. willodap-c — `userspace/willodap-c/`
- DAP server bridging to GDB for native ELF; fallback `willodap-mi`-class for older toolchains.

### T4. willodap-py — `userspace/willodap-py/`
- Python `pdb`/`debugpy`-class adapter; runs Python interpreter inside §41 container by default.

### T5. LSP orchestration — `userspace/willedit/lsp/`
- LanguageServer pool; capability negotiation; multi-root workspaces.
- Health monitor; auto-restart with backoff.

### T6. Tree-sitter integration — `userspace/willedit/syntax/`
- Per-language grammar packs; incremental parse on edits via `Tree::edit`.
- Highlight queries shipped per grammar.

### T7. Workspace search — `userspace/willo-rg/`
- Standalone `ripgrep`-class CLI (Rust regex + memmap + parallel walk).
- Editor wraps with streaming UI.

### T8. Git client — `userspace/willedit-git/`
- Wrapper over `git2-rs` or libgit2 port; UI panes for status / diff / log / branches.
- All git ops sandboxed in §39 profile.

### T9. Build orchestration — `userspace/willedit/build/`
- Task runner spec (`/etc/willedit/tasks.toml`) + per-workspace `tasks.toml`.
- ANSI-aware output panel; terminal embedding (§34 PTY) for interactive tasks.

## New / modified files

| Path | Change |
| --- | --- |
| `userspace/willedit/dap/` | **new** |
| `userspace/willedit/lsp/` | extend (multi-root) |
| `userspace/willedit/syntax/` | extend (incremental + queries) |
| `userspace/willedit/build/` | **new** |
| `userspace/willodap-{rust,c,py}/` | **new** |
| `userspace/willo-rg/` | **new** |
| `userspace/willedit-git/` | **new** |

## Tests to add

- `userspace/willedit/tests/dap_breakpoint.rs` — set+hit conditional breakpoint.
- `userspace/willedit/tests/lsp_multiroot.rs` — two-root workspace; jump-to-def crosses roots.
- `userspace/willedit/tests/workspace_search.rs` — 100k file fixture; <100 ms.
- `userspace/willedit/tests/syntax_incremental.rs` — parse stability after small edit.
- `userspace/willedit-git/tests/diff_status.rs` — status + stage + commit round-trip.

## Risks & open questions

- **DAP ↔ GDB stub coupling** — mismatched DWARF can confuse step-over; lock toolchain via §33.
- **LSP server zoo** — ship the top 8: rust-analyzer, clangd, pyright, gopls, typescript-ls, marksman, lua-ls, bash-language-server.
- **Tree-sitter grammar updates** — pin via §19; bumping is breaking for highlight themes.
- **Workspace search on encrypted FS** — bench vs §38 plain FS; OK at >500 MB/s.
- **Git remote operations** — credentials via §38 SecretService; never via env.
- **Editor as compositor client** — don't grow into a desktop shell; stay focused on editing.
