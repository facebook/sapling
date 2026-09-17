# Mononoke reference

Oncall: `scm_server_infra` · Language: Rust

Use this reference to locate Mononoke protocol, API, repository, derived-data,
and storage code.

## Start here

| Need | Path | Primary symbols |
|------|------|-----------------|
| EdenAPI or SLAPI request | `fbcode/eden/mononoke/servers/slapi/` | `SaplingRemoteApiHandler` |
| Source Control Service request | `fbcode/eden/mononoke/servers/scs/` | `SourceControlServiceImpl` |
| Git protocol | `fbcode/eden/mononoke/servers/git/` | `GitServerContext` |
| Git LFS | `fbcode/eden/mononoke/servers/lfs/` | `LfsServerContext` |
| High-level repository API | `fbcode/eden/mononoke/mononoke_api/` | `RepoContext`, `ChangesetContext` |
| Repository facets | `fbcode/eden/mononoke/repo_attributes/` | `#[facet::container]` components |
| Derived data | `fbcode/eden/mononoke/derived_data/` | `BonsaiDerivable` |
| Storage and decorators | `fbcode/eden/mononoke/blobstore/` | `Blobstore` |
| Cross-repository sync | `fbcode/eden/mononoke/megarepo_api/` | `MegarepoApi` |
| Operational tooling | `fbcode/eden/mononoke/tools/admin/` | `AdminArgs` |

## Request model

```text
SLAPI, SCS, Git, or LFS server
  -> mononoke_api
  -> repository facets
  -> prefix -> cache -> multiplex -> pack -> storage blobstores
```

Operations carry `CoreContext` for logging, authorization, and telemetry.
Repository capabilities are injected as facets and expressed through trait
bounds. Blobstore decorators do not overwrite existing keys by default.

For BUCK conventions, OSS boundaries, and focused validation,
read `fbcode/eden/mononoke/.claude/CLAUDE.md` when those details affect the task.
