---
name: 'eden_research'
description: >
  Load FIRST before searching code in eden/. Contains component-to-file maps,
  key type names, and architecture diagrams for EdenFS, Sapling, and Mononoke.
  Tells you exactly which files to read for any task. References contain
  per-component quick reference tables.
oncalls:
  - 'scm_client_infra'
  - 'scm_server_infra'
  - 'sapling'
apply_to_regex: 'eden/.*'
---

# Eden Research

## Overview

**Oncalls**: `scm_client_infra` · `scm_server_infra` · `sapling`

Eden is Meta's source control ecosystem comprising three major sub-projects that together provide fast, scalable version control for massive monorepos. EdenFS lazily virtualizes the filesystem, Sapling provides the CLI, and Mononoke serves as the backend.

## When to Use

- Researching how Eden subsystems work or connect
- Understanding cross-project data flows (Sapling ↔ EdenFS ↔ Mononoke)
- Navigating the Eden codebase to find the right code for a task
- Understanding the shared libraries in `eden/common/`

## Architecture

```
Developer's machine                          Meta's servers
┌──────────────────────────────────┐        ┌──────────────────────────────────┐
│  Sapling CLI (eden/scm/)         │        │  Mononoke (eden/mononoke/)       │
│  Rust + Python │ oncall: sapling │        │  Rust │ oncall: scm_server_infra │
│  User commands: sl status,       │        │                                  │
│  sl commit, sl goto, sl rebase   │        │  ┌────────────────────────────┐  │
│         │                        │        │  │ SLAPI Server               │  │
│         ▼                        │        │  │ (servers/slapi/)           │  │
│  EdenFS (eden/fs/)               │        │  │ EdenAPI REST endpoints     │  │
│  C++ │ oncall: scm_client_infra  │        │  └───────────┬────────────────┘  │
│  Virtual filesystem daemon       │        │              │                   │
│  Lazy-loads files on demand      │        │  ┌───────────▼────────────────┐  │
│         │                        │        │  │ Mononoke API               │  │
│    ┌────▼─────────────────┐      │        │  │ (mononoke_api/)            │  │
│    │ SaplingBackingStore   │─── EdenAPI ──▶│  └───────────┬────────────────┘  │
│    │ (Rust FFI bridge)     │     │        │              │                   │
│    └──────────────────────┘      │        │  ┌───────────▼────────────────┐  │
│                                  │        │  │ Blobstore Stack            │  │
│  ┌──────────────────────────┐    │        │  │ prefix→cache→multiplex     │  │
│  │ eden/common/             │    │        │  │ →pack→sqlblob/manifold     │  │
│  │ Shared: ImmediateFuture, │    │        │  └────────────────────────────┘  │
│  │ PathFuncs, FaultInjector,│    │        │                                  │
│  │ TraceBus, RefPtr         │    │        │  ┌────────────────────────────┐  │
│  └──────────────────────────┘    │        │  │ SCS Server (Thrift)        │  │
│                                  │        │  │ (servers/scs/)             │  │
└──────────────────────────────────┘        └──────────────────────────────────┘
```

Also: `eden/integration/` (Python, end-to-end tests), `eden/addons/` (TypeScript, Interactive Smartlog UI).

## Cross-Cutting Data Flows

### File Read (user reads a file in an Eden checkout)

```
User process reads file
  → Kernel VFS → EdenFS FsChannel (FUSE/NFS/PrjFS)
    → Dispatcher → InodeMap → FileInode
      → materialized? → Overlay (local disk)
      → non-materialized? → BlobAccess → ObjectStore
        → BlobCache hit? → return
        → miss → SaplingBackingStore (Rust FFI)
          → EdenAPI HTTP → Mononoke SLAPI Server
            → Mononoke API → Blobstore Stack → storage
```

### Commit (user runs `sl commit`)

```
User runs `sl commit`
  → Sapling CLI (Rust command dispatch)
    → workingcopy lib: scan EdenFS for changes (Thrift: getScmStatusV2)
    → EdenFS: diff working copy vs parent commit
    → Sapling: create commit object
    → EdenAPI client → Mononoke: upload trees + files + commit
```

### Checkout (user runs `sl goto`)

```
User runs `sl goto <rev>`
  → Sapling CLI → EdenFS Thrift: checkOutRevision()
    → EdenServiceHandler → EdenMount::checkout()
      → rootInode->checkout() → computeCheckoutActions(): diff old→new tree
      → CheckoutAction::run(): materialize/dematerialize inodes
      → FsChannel: invalidate kernel caches
```

## Cross-Cutting Patterns

### Thrift Boundaries

EdenFS exposes a Thrift API defined in `eden/fs/service/eden.thrift` and `streamingeden.thrift`. Conventions:
- Wrap arguments in `*Request` structs, return `*Response` structs
- Use `MountId` (containing `mountPoint` path) as the first field
- When adding/modifying: update mock service at `eden/fs/cli_rs/edenfs-client/src/client/mock_service.rs`

### EdenAPI/SLAPI Protocol

HTTP-based API between Sapling/EdenFS clients and Mononoke server. Endpoints defined in `eden/mononoke/servers/slapi/`. Client library at `eden/scm/lib/edenapi/`. See the [CREATING_ENDPOINTS skill](../CREATING_ENDPOINTS/SKILL.md) for adding new endpoints.

## Output Format

- **"What is X?"** → Short paragraph + key file pointer (`path/to/File.cpp:line`)
- **"How does X work?"** → Explanation with ASCII flow diagram
- **"Trace X"** → Numbered list of `file_path:function_name()` in execution order
- **"How do X and Y connect?"** → ASCII diagram showing boundary + interface files
- **"Where is X?"** → Table of file paths with one-line descriptions

Always include concrete code pointers. Never give a pure prose answer without referencing actual code.

## How to Research

Follow these tiers in order. Stop at the first tier that answers the question.

**Tier 1 — Answer from skill content (seconds):**
Check this skill's Architecture, Cross-Cutting Data Flows, and Component Overview sections for high-level "What is X?", "Where is X?", and "How do X and Y connect?" questions. If the question is about a specific component, load the appropriate reference from `references/` (see Detailed References section below).

**Tier 2 — Read source files directly (under 1 min):**
Use the Detailed References table below to load the appropriate reference, which contains Component Overview and Quick Reference tables for the relevant component. Then read source files directly. Also check the relevant CLAUDE.md (`eden/fs/.claude/CLAUDE.md`, `eden/scm/.claude/CLAUDE.md`, etc.) for additional architecture context. This handles most "How does X work?", "Trace X", and implementation questions.

**Tier 3 — Spawn subagents for deep exploration (3-5 min):**
Only when Tiers 1-2 are insufficient — e.g., tracing across multiple subsystems, understanding rarely-documented internals, or questions about code with no skill/CLAUDE.md coverage. Spawn one `meta_codesearch:code_search` subagent per subsystem, launched in parallel. If a subagent fails or returns nothing useful, fall back to this skill's static content and point to `eden/fs/docs/`. Don't retry.

## Detailed References

For component-specific information (component overview, quick reference, key concepts), load the appropriate reference from `eden/.llms/skills`:

| Component | Reference | When to Load |
|-----------|-----------|--------------|
| **EdenFS** | [`references/edenfs.md`](references/edenfs.md) | Inodes, ObjectStore, FUSE/NFS/PrjFS, Thrift service |
| **Sapling** | [`references/sapling.md`](references/sapling.md) | CLI commands, Rust libraries, Python interop, .t tests |
| **Mononoke** | [`references/mononoke.md`](references/mononoke.md) | SLAPI/SCS/Git servers, blobstore, derived data, facets |
| **Integration** | `eden/integration/.claude/CLAUDE.md` | End-to-end tests with real EdenFS daemon |
| **Endpoints** | [`../CREATING_ENDPOINTS/SKILL.md`](../CREATING_ENDPOINTS/SKILL.md) | Adding new EdenAPI/SLAPI endpoints |
