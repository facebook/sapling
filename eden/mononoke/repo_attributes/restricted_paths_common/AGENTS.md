# restricted_paths_common

The config-backed restriction source for Path ACLs, plus the types derived-data
crates need. It is separate from `restricted_paths` so derivation code
(`mercurial_derivation`, `fsnodes`, `content_manifest_derivation`,
`derived_data_manager`, `repo_derived_data`) can use them without a dependency
cycle (`restricted_paths` depends on `repo_derived_data`). Read
`repo_attributes/restricted_paths/AGENTS.md` first.

`restricted_paths` re-exports everything here (`pub use restricted_paths_common::*`).

## Modules

| Module | Owns |
|---|---|
| `config_based.rs` | `RestrictedPathsConfigBased` facet: the parsed `RestrictedPathsConfig`, the manifest-id store and the optional cache. Synchronous config lookups: exact root (`get_metadata_for_path`, `is_restriction_root`, `get_acl_for_path`), prefix (`get_acl_for_path_prefix`). |
| `manifest_id_store.rs` | `RestrictedPathsManifestIdStore` trait, the SQL store, the no-op store, `ManifestType`, `RestrictedPathManifestIdEntry`, `ManifestIdStoreWriteCallsite`. |
| `cache.rs` | `RestrictedPathsManifestIdCache`: an in-memory copy of the store (`RestrictedPathsManifestIdStoreConfig`). |
| `schemas/sqlite-restricted-paths-manifest-ids.sql` | Table `restricted_paths_manifest_ids`: unique on `(repo_id, manifest_type, manifest_id, path_hash)` and on `(repo_id, id)`. |

## The manifest-id store

Manifests are content-addressed and carry no path. The store maps manifest ids
to configured restriction roots so manifest accesses can be matched to them.

## Invariants

- **Writers** pass their `ManifestIdStoreWriteCallsite`. A new writer needs a
  new callsite variant.
- **Verification tools** use `NoopRestrictedPathsManifestIdStore`.
- **Guard before querying** with `config().is_empty()` (not
  `may_have_restricted_paths()`, which also covers `.slacl`). Store lookups
  sit on per-tree hot paths.
- **Schema changes** need the server database change as well as the SQLite
  schema file.

## Test

```bash
buck2 test @fbcode//mode/opt fbcode//eden/mononoke/repo_attributes/restricted_paths_common/...
```

Update this file when you add a module, a writer callsite or a store method, or
change an invariant above.
