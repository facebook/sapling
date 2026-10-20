# acl_manifest (AclManifest derived data)

`RootAclManifestId` is the derived data type (`DerivableType::AclManifests`)
that turns the `.slacl` files of a commit into a sparse tree the server can
query for Path ACL restrictions. It is one of the two restriction sources; the
other is the repo config. Design overview: `repo_attributes/restricted_paths/AGENTS.md`.

## Data model (`mononoke_types/src/acl_manifest.rs`)

- `AclManifest`: one node per directory that is a restriction root **or** has
  a restricted descendant (a "waypoint"). Unrestricted subtrees have no node.
  - `restriction`: `Restricted(AclManifestRestriction)` or `Unrestricted`.
  - `subentries`: a `ShardedMapV2` of child directories (with `is_restricted`
    and `has_restricted_descendants` flags) and the directory's own ACL file
    leaf. Ordinary files have no entries.
  - The rollup `has_restricted` lets a parent compute
    `has_restricted_descendants` without enumerating subentries.
- `AclManifestEntryBlob`: the parsed ACL file (`repo_region_acl`,
  `permission_request_group`, `rollout_allowlist_group`), stored separately and
  content-addressed, so identical files share one blob.
- `AclManifest::empty_id` computes the empty manifest's id without a blobstore
  write.
- Mapping key: `derived_root_aclmf.<prefix><changeset id>`, where the prefix
  comes from the repo's `DerivedDataTypesConfig.mapping_key_prefixes`.

## Derivation (`derive.rs`)

- `derive_single` uses the shared `derive_manifest` primitive. Only ACL file
  changes are fed to it, so cost scales with ACL changes and their depth, not
  with the size of the commit. Unchanged subtrees are reused.
- `get_acl_file_changes` collects all ACL file changes from the bonsai,
  including deletes implied by directory replacement.
- `derive_from_scratch` (`DerivableUntopologically`, for backfills) finds every
  ACL file with BSSMV3, resolves contents through ContentManifests, and derives
  with no parents.
- `pipeline.rs` implements `PipelineDerivable` (per-path stages, no finalize).
- `fetch_and_parse_acl_file` fetches and parses ACL files with
  `restricted_paths_acl_file::parse_acl_file`.

## Inline nodes for HgAugmented manifests (`upload.rs`)

`acl_node_for_directory` builds one directory's node from that directory's own
contents: its ACL file and its children's nodes. The HgAugmented upload path
(`derived_data/mercurial_derivation/src/upload_augmented_manifest.rs`) uses it
to attach ACL data to each augmented tree. HgAugmented derivation and backfill
read the canonical `RootAclManifestId` instead.

- Inputs are the directory's full state, not a delta, so a deleted restricted
  subtree cannot be carried forward by accident.
- It must produce exactly the node the canonical derivation produces for the
  same directory, including `None` for a directory with neither a restriction
  nor a restricted descendant. The
  `test_acl_node_for_directory_matches_derivation_*` tests enforce this; extend
  them with any new case.

## Rules for changes

- Changing the encoding of `AclManifest` or `AclManifestEntryBlob` changes
  every derived id. Roll it out with a new mapping key prefix in the repos'
  derived data config; never edit the `derived_root_aclmf.` literal, and never
  re-derive in place (production blobstore puts do not overwrite). A test in
  `mononoke_types` pins `AclManifestEntryBlob`'s id: new optional fields must
  not change the encoding when unset.
- Use `derive_manifest` and `ShardedMapV2` subtree reuse; do not add a custom
  traversal.
- Restriction lookups against this data live in
  `repo_attributes/restricted_paths/src/restriction_info.rs`; keep them in step
  with any model change.

## Test

```bash
buck2 test @fbcode//mode/opt fbcode//eden/mononoke/derived_data:acl_manifest
```

Tests cover incremental add/remove/modify, nested restrictions, implicit
deletes, merges, from-scratch derivation, exact blobstore
get/put counts (cost scaling), and inline-node parity.

Update this file when the data model, derivation rules or the inline-node
contract change.
