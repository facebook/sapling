# Path ACLs (restricted paths)

Path ACLs (also called restricted paths, or code tenting) restrict **read**
access to directories in a Mononoke repo. A restricted directory is a
**tent**. Each tent is guarded by one REPO_REGION ACL, and the server decides
for every access whether the caller may read it. This crate is the core of the
feature: restriction lookup, authorization, enforcement, and access logging.
Every other Path ACL crate either feeds it or calls it.

Paths below are relative to `fbcode/eden/mononoke/`. Meta-internal technical
details (configuration locations, logging configuration, Meta-only hooks),
where available, are in `repo_attributes/restricted_paths/facebook/AGENTS.md`.

## Keep these files current

Each Path ACL directory with an `AGENTS.md` documents its own design. When you
change a behaviour, invariant, config field or module that one of them
describes, update it in the same diff. Record new design decisions, with the
reason, in [Design decisions](#design-decisions) or in the narrower file they
belong to. `CLAUDE.md` is a symlink to `AGENTS.md`: edit `AGENTS.md`.

## Glossary

| Term | Meaning |
|---|---|
| Tent / restriction | A restricted directory tree guarded by one REPO_REGION ACL. One access can match several restrictions (nested or multiple tents). |
| Restriction root | The directory that carries the restriction: the directory holding the `.slacl` file, or the configured path. |
| Restriction source | Where a restriction is declared: the repo config (`path_restriction_metadata`, plus the manifest-id store for manifest accesses), or `.slacl` files read through the AclManifest derived data. |
| `AclManifestMode` | Selects the restriction sources (`metaconfig_types::AclManifestMode`). |
| Authorization | Whether the caller may read a restriction. |
| Permission request group | The group a denied caller is told to request. |
| Rollout allowlist group | Per-tent group authorized to read that tent (`rollout_allowlist_group`). |
| Tooling allowlist group | Repo-wide group authorized to read every tent. |
| Admin group | `admin_bypass_group`: repo-wide break-glass group authorized to read every tent and to change any `.slacl`. |
| Enforcement configuration | The enforcement fields of `RestrictedPathsConfig`. |

## Where the code lives

| Area | Path | What it owns |
|---|---|---|
| Core facet | `repo_attributes/restricted_paths/` (this crate) | `RestrictedPaths` facet, lookups, authorization, enforcement, access logging, `.slacl` change validation |
| Config-backed source | `repo_attributes/restricted_paths_common/` | `RestrictedPathsConfigBased`, manifest-id store and cache, `ManifestType` |
| `.slacl` parser | `repo_attributes/restricted_paths_acl_file/` | `parse_acl_file` |
| AclManifest derived data | `derived_data/acl_manifest/` | The sparse tree of restriction roots derived from `.slacl` files |
| Config types | `metaconfig/types/src/lib.rs` | `RestrictedPathsConfig`, `PathRestrictionMetadata`, `AclManifestMode`, the enforcement configuration types, `RestrictedPathsAclFile`, `parse_bare_group_name` |
| Config parsing | `metaconfig/parser/src/convert/repo.rs` | Raw repo config to the types above |
| Manifest-id store | derivation crates | Maintain the manifest-id store (`ManifestIdStoreWriteCallsite`) |
| ACL data in HgAugmented manifests | `derived_data/mercurial_derivation/src/upload_augmented_manifest.rs` | Builds ACL nodes with `acl_manifest::acl_node_for_directory` |
| Enforcement callers | `mononoke_api/`, `mononoke_api_hg/` | Call the enforcement entry points |
| API-layer types | `mononoke_api/src/restricted_paths.rs`, `mononoke_api/src/errors.rs` | `PathAccessInfo`, `RestrictedPathsPolicy`, `MononokeError::RestrictedPathsAuthorizationError` |
| Changeset creation | changeset creation paths | Call `validate_acl_file_changes_in_changesets` |
| SCS | `servers/scs/scs_methods/src/methods/commit_restricted_paths.rs`, `servers/scs/scs_errors/` | Restricted-paths metadata methods, denial error mapping |
| Admin CLI | `tools/admin/src/commands/restricted_paths.rs` | Manifest-id store administration |
| `scsc` CLI | `clients/scsc/src/commands/restricted_paths.rs` | Client commands for the metadata methods |

## Architecture

This crate is a repo attribute: it must not depend on `features/`, `tools/` or
server business logic (it uses the SLAPI `context` crate only for
`CoreContext`). Servers call `mononoke_api`, which calls this crate.

```
server handlers
        |
mononoke_api, mononoke_api_hg   (RestrictedPathsPolicy)
        |
restricted_paths (this crate)   lib.rs: entry points and enforcement orchestration
   |-- restriction_check.rs     authorization and enforcement outcomes
   |-- restriction_info.rs      source lookups (config, AclManifest), mode dispatch
   `-- access_log.rs            access-log rows
        |                        |
restricted_paths_common     acl_manifest derived data -> restricted_paths_acl_file
```

- `lib.rs`: the facet, public lookup methods, the `.slacl` validator, and the
  enforcement entry points `spawn_enforce_restricted_path_access` and
  `spawn_enforce_restricted_manifest_access`. Both delegate to
  `spawn_enforce_restricted_access`, the single enforcement flow. Put new
  enforcement logic there, not in the entry points.
- `restriction_check.rs`: per-restriction authorization, enforcement outcomes
  and their aggregation across sources, `EnforcementDecision`,
  `SharedFetchHandle`, and the logging aggregate `SourceRestrictionSummary`.
- `restriction_info.rs`: restriction lookups from each source, and the
  per-mode dispatch.
- `access_log.rs`: builds access-log rows (raw Scuba, plus the schematized
  logger in fbcode builds).

## Enforcement

- Every access check goes through `spawn_enforce_restricted_access`, which
  evaluates the enforcement configuration once, reads each restriction source
  once through shared handles (logging reuses the same handles), and returns
  `RestrictedPathsError::AuthorizationError` on a denial.
- Fail closed: a failed source read or ACL check returns an error, never an
  allow.
- When several restrictions deny, the reported `DenyingRestriction` is chosen
  deterministically, so error messages are stable.
- Logging receives the computed `EnforcementDecision`; it never recomputes it.

## Restriction sources and `AclManifestMode`

- `RestrictedPaths::new` checks that the derived data types the configured
  mode needs are enabled.
- When both sources are used, an access is denied if either denies, and
  metadata lookups return the union (for the same root, the AclManifest entry
  wins).
- Use `may_have_restricted_paths()`, not config emptiness, to decide whether a
  repo can have restrictions: it accounts for `.slacl` files.

## Authorization

Each matched restriction is authorized on its own
(`check_restriction_authorization_with_acl`). The caller is authorized for a
restriction through `read` on its REPO_REGION ACL, its rollout allowlist group,
the tooling allowlist group, or the admin group. Admin-group authorization is
recorded separately from ACL access.

- An access is denied if **any** of its restrictions denies. Nested and
  multiple tents are conjunctive: access to one tent never authorizes another.
- An absent group never grants access.
- ACL-provider and group-membership errors propagate (fail closed).
- `SourceRestrictionSummary` aggregates checks for logging only; enforcement
  uses the per-restriction results. Keep the two separate.
- `.slacl` changes need the `maintainers` action on the ACL
  (`has_maintainer_access_to_repo_region`) or the admin group.

## Metadata APIs

`get_path_restriction_info`, `get_path_restriction_root_info`,
`find_restricted_descendants`, `is_restricted_path`, `is_restriction_root`,
`get_manifest_restriction_info`, `is_restricted_manifest`, and the checks
`get_path_restriction_check`, `get_path_restriction_root_check`,
`get_manifest_restriction_check` answer "what is restricted here, and may this
caller read it". Their implementations are in `restriction_info.rs` and
`restriction_check.rs`.

## `.slacl` change validation

`validate_acl_file_changes_in_changesets` requires `maintainers` on the
restriction's ACL, or the admin group, to change a `.slacl`. Do not
re-implement its checks at call sites.

## Access logging

Rows are built in `access_log.rs` and written to `ACCESS_LOG_SCUBA_TABLE` and
the schematized logger. A new or renamed column must be added to both.

## Design decisions

Record new decisions here with their reason.

1. **Server-side enforcement.** The server decides every access; enforcement
   does not depend on client behaviour. Client-side checks are UX only.
2. **Manifest enforcement.** Content-addressed manifests carry no path, so
   manifest accesses are matched to restrictions through the manifest-id store
   and AclManifest data.
3. **Fail closed.** A failed source read or ACL check is an error, never an
   allow.
4. **Authorization is recorded.** The access log records the caller's
   authorization.
5. **One enforcement flow, evaluated once.** Path and manifest enforcement
   share `spawn_enforce_restricted_access`.
6. **Two restriction sources.** Config and `.slacl`; when both are used, an
   access is denied if either denies.
7. **Cheap guards before I/O.** Skip the manifest-id store when the config has
   no restricted paths, because store lookups run per tree.

## Changing this code safely

- This is the hot read path of every surface. A bug either serves restricted
  content or denies unrestricted content to everyone. Prefer small, reversible
  diffs.
- Never add fallback defaults to JustKnob reads, and never default an ACL check
  or source lookup to allow.
- When you change authorization, change the per-restriction checks and the
  logging summary deliberately, and test both.
- A behaviour change updates the unit tests, `test/`, the integration tests
  and, if outcomes change on purpose, the release parity gates.
- Split refactors from behaviour changes; add tests that pin the existing
  behaviour in a diff below the behaviour change.

## Testing

```bash
arc rust-check fbcode//eden/mononoke/repo_attributes/restricted_paths:restricted_paths
buck2 test @fbcode//mode/opt fbcode//eden/mononoke/repo_attributes/restricted_paths/...
```

- Unit tests: inline in `lib.rs`, and `src/<module>/tests.rs` for the other
  modules, with helpers in `src/test_utils.rs`.
- `test/`: the scenario test target `restricted_paths_test` (see
  `test/AGENTS.md`).
- New tests start with a `What it tests:` / `Expected:` comment.
