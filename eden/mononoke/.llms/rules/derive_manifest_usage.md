---
name: derive-manifest-usage
metadata:
  oncalls: ['scm_server_infra']
  strict: true
  apply_to_path: 'eden/mononoke/.*\.rs$'
  apply_to_content: 'BonsaiDerivable|bounded_traversal|derive_single|ManifestOps|TreeInfoSubentries|derive_from_parents|create_tree.*create_leaf'
---

# Prefer `derive_manifest` for Tree-Structured Derived Data

**Severity: MEDIUM**

## When to Flag

- A new derived data type that implements its own `bounded_traversal` (or manual recursion) to build a tree of directories and files, instead of calling `derive_manifest` from the manifest crate (`eden/mononoke/manifest/src/derive.rs`). The manifest crate already handles multi-parent merge semantics, subtree reuse, implicit deletes (file replacing directory), and ShardedMapV2 trie-level optimization.
- Code that duplicates merge resolution logic -- e.g., checking whether parents agree, resolving first-parent-wins, or detecting reusable subtrees -- when `derive_manifest` provides this out of the box via its `create_tree` and `create_leaf` callbacks.
- Missing batch derivation support for linear stacks. If the type uses `derive_manifest`, it can also use `derive_manifests_for_simple_stack_of_commits` for significantly faster derivation of non-merge commit chains.

## Do NOT Flag

- Derived data types whose data model doesn't fit `Entry<TreeId, Leaf>` -- e.g., Deleted Manifest (tracks deletion state with unode co-input), History Manifest (tracks history chains with linknodes and deleted-vs-live state), or DBCM (tracks directory cluster membership without file leaves). These have legitimate reasons for custom traversal.
- Non-tree derived data with no recursive directory structure (ChangesetInfo, Filenodes, InferredCopyFrom, etc.).
- Code that already uses `derive_manifest`, `derive_manifest_with_known_entries`, `derive_manifests_for_simple_stack_of_commits`, or `derive_manifest_from_predecessor`.

## Recommendation

Use `derive_manifest` with custom `create_tree` and `create_leaf` callbacks. See fsnodes, skeleton_manifest, unodes, content_manifest, and acl_manifest for canonical examples. For manifest type conversions (e.g., non-sharded to sharded), use `derive_manifest_from_predecessor`.
