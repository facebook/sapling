# Sapling Architecture

**Oncall**: `sapling` · **Language**: Rust + Python

## Component Overview

| Component | Path | Key Types | Purpose |
|-----------|------|-----------|---------|
| Binary entry | `exec/hgmain/` | `main.rs` | Entry point — dispatches to Rust or Python |
| Rust commands | `lib/commands/` | `define_flags!`, `run()` | Rust command implementations |
| Python commands | `sapling/commands/` | `@command` decorator | Python command implementations |
| Rust libraries | `lib/` | — | Pure Rust libs (no Python deps) |
| Python-Rust bindings | `saplingnative/bindings/` | `py_class!` macro | ~70 Rust-to-Python binding modules |
| Python core | `sapling/` | `dispatch.py`, `extensions.py` | Commands, extensions, repo operations |
| EdenFS FFI | `lib/backingstore/` | BackingStore | C++ FFI so EdenFS can fetch data |
| Tests | `tests/` | `.t` files, `tinit.sh` | Test suite |

## Architecture

```
Command dispatch:
  exec/hgmain/ → Rust command table (lib/commands/src/run.rs)
    → found? → execute in Rust
    → not found / fallback!() → init Python (HgPython) → sapling/dispatch.py

Dependency flow: Python → Rust bindings (saplingnative/) → Rust libs (lib/)
```

## Key Concepts

- **Identity system**: Binary supports SL/HG/SL_GIT identities — controls `.sl/` vs `.hg/` directory, CLI name
- **EdenFS mode**: Communicates via Thrift when working copy is virtualized; `lib/backingstore/` provides C++ FFI
- **Python embedding**: All Python source is compiled into the Rust binary but live-reloads from fbsource without recompiling

## Quick Reference

| Task | Where to Start |
|------|---------------|
| Add Rust command | `lib/commands/commands/` — `define_flags!` + `run()` |
| Add Python command | `sapling/commands/` — `@command` decorator |
| Add Rust-Python binding | `saplingnative/bindings/modules/py*/` — `py_class!` macro |
| Debug command dispatch | `lib/commands/src/run.rs` → Python fallback |
| Modify EdenFS FFI | `lib/backingstore/` |

## Common Mistakes

| Mistake | Correct Approach |
|---------|-----------------|
| Editing `Cargo.toml` | They're generated from BUCK — edit BUCK files |
| Using `$ hg` in new .t tests | Use `$ sl` (Sapling branding) |
| Adding Python dep to Rust lib | Rust libs in `lib/` must be pure Rust — use `saplingnative/bindings/` |

For detailed library reference, build commands, .t test format, identity system, extensions, and conventions, see `eden/scm/.claude/CLAUDE.md`.
