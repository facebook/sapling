# Sapling reference

Oncall: `sapling` · Languages: Rust and Python

Use this reference to locate command, library, working-copy, and remote-access
code in `eden/scm/`.

## Start here

| Need | Path | Primary symbols or pattern |
|------|------|----------------------------|
| Binary startup | `fbcode/eden/scm/exec/hgmain/` | `main.rs` |
| Rust command dispatch | `fbcode/eden/scm/lib/commands/src/run.rs` | command table, `fallback!()` |
| Rust command implementation | `fbcode/eden/scm/lib/commands/commands/` | `define_flags!`, `run()` |
| Python command implementation | `fbcode/eden/scm/sapling/commands/` | `@command` |
| Extensions | `fbcode/eden/scm/sapling/ext/` | extension modules |
| Rust-to-Python bridge | `fbcode/eden/scm/saplingnative/bindings/` | `py_class!` modules |
| Working-copy state | `fbcode/eden/scm/lib/workingcopy/`, `fbcode/eden/scm/lib/treestate/` | working copy and treestate APIs |
| Remote EdenAPI client | `fbcode/eden/scm/lib/edenapi/` | EdenAPI requests |
| EdenFS C++ bridge | `fbcode/eden/scm/lib/backingstore/` | backing-store FFI |
| CLI tests | `fbcode/eden/scm/tests/` | `.t` files, `tinit.sh` |

## Dispatch model

```text
exec/hgmain
  -> Rust command table
  -> Rust implementation, or `fallback!()`
  -> embedded Python -> sapling/dispatch.py

Python -> saplingnative bindings -> Rust libraries
```

The binary supports `SL`, `HG`, and `SL_GIT` identities. In an EdenFS checkout,
Sapling delegates working-copy operations over Thrift. Embedded Python reloads
from fbsource during local development.

For generated Cargo manifests, read `fbcode/eden/.claude/CLAUDE.md`.
For identity conventions and focused test commands, read
`fbcode/eden/scm/.claude/CLAUDE.md` when those details affect the task.
