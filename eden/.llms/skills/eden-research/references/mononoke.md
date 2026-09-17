# Mononoke Architecture

**Oncall**: `scm_server_infra` · **Language**: Rust

## Component Overview

| Component | Path | Key Types | Purpose |
|-----------|------|-----------|---------|
| SLAPI Server | `servers/slapi/` | SaplingRemoteApiHandler | Primary EdenAPI HTTP server for Sapling/EdenFS |
| SCS Server | `servers/scs/` | SourceControlServiceImpl | Thrift interface (`source_control.thrift`) |
| Git Server | `servers/git/` | GitServerContext | Git protocol over HTTP |
| LFS Server | `servers/lfs/` | LfsServerContext | Large file storage for Git |
| Blobstore | `blobstore/` | Blobstore trait | Storage abstraction (prefix→cache→multiplex→pack→storage) |
| Derived Data | `derived_data/` | BonsaiDerivable trait | Computed data derived from commits |
| Mononoke API | `mononoke_api/` | RepoContext, ChangesetContext | High-level API layer |
| Megarepo | `megarepo_api/` | MegarepoApi | Cross-repo sync for monorepo |
| Admin CLI | `tools/admin/` | AdminArgs | Operational/debugging tool |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ Mononoke Server                                             │
│                                                             │
│ ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│ │ SLAPI/      │  │ SCS Server  │  │ Git/LFS Servers     │ │
│ │ EdenAPI     │  │ (Thrift)    │  │                     │ │
│ └──────┬──────┘  └──────┬──────┘  └──────┬──────────────┘ │
│        │                │                │                 │
│        └────────────────┴────────────────┘                 │
│                         │                                  │
│        ┌────────────────▼────────────────┐                 │
│        │ Mononoke API (mononoke_api/)   │                 │
│        │ RepoContext, ChangesetContext  │                 │
│        └────────────────┬────────────────┘                 │
│                         │                                  │
│        ┌────────────────▼────────────────┐                 │
│        │ Repo (40+ facets)              │                 │
│        │ #[facet::container]            │                 │
│        └────────────────┬────────────────┘                 │
│                         │                                  │
│        ┌────────────────▼────────────────┐                 │
│        │ Blobstore Stack                │                 │
│        │ prefixblob→cacheblob           │                 │
│        │ →multiplexedblob→packblob      │                 │
│        │ →sqlblob/manifoldblob          │                 │
│        └────────────────────────────────┘                 │
└─────────────────────────────────────────────────────────────┘
```

## Key Concepts

- **Facet-based DI**: `Repo` uses `#[facet::container]` with 40+ facets. Functions declare requirements via trait bounds.
- **CoreContext pattern**: All operations thread `CoreContext` for logging, auth, telemetry. Tests use `CoreContext::test_mock(fb)`.
- **Blobstore stack**: Decorator chain — prefix → cache → multiplex → pack → storage. Default: never overwrites existing keys.

## Quick Reference

| Task | Where to Start |
|------|---------------|
| Add SLAPI endpoint | `servers/slapi/slapi_service/src/handlers/` — implement `SaplingRemoteApiHandler` |
| Add derived data type | `derived_data/` — implement `BonsaiDerivable` trait |
| Add blobstore decorator | `blobstore/` — implement `Blobstore` trait |
| Add facet | `repo_attributes/` — create new facet crate |
| Debug request | `mononoke_api/` — RepoContext methods |

For detailed patterns, error handling, testing, and build commands, see `eden/mononoke/.claude/CLAUDE.md` and `eden/mononoke/docs/`.
