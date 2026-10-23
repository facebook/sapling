---
name: sharded-map-v2-correctness
metadata:
  oncalls: ['scm_server_infra']
  strict: true
  apply_to_path: 'eden/mononoke/.*\.rs$'
  apply_to_content: 'ShardedMapV2|WEIGHT_LIMIT|sharded_map_v2|from_entries_and_partial_maps|LoadableShardedMapV2Node'
---

# ShardedMapV2 Correctness

**Severity: MEDIUM**

## What to Look For

- ShardedMapV2 subtree reuse that only expands one level of the trie
- WEIGHT_LIMIT values without justification
- Merge logic that enumerates all entries when partial subtree reuse is possible

## When to Flag

- Code that expands a ShardedMapV2 by only the first byte to find reusable subtrees, without recursing deeper. A single-level expansion misses optimization opportunities -- if parents share a subtree at a deeper prefix, it should be reused without enumerating individual entries. Check the manifest crate (`from_entries_and_partial_maps`) for the correct recursive pattern.
- A `WEIGHT_LIMIT` constant set to an arbitrary value (or left as `TODO`) without a comment explaining why that value was chosen. The weight limit controls how the sharded map splits its entries across blobs and directly affects read/write amplification and blob sizes.
- Merge logic that calls `into_entries()` on all parent ShardedMapV2 nodes unconditionally. When parents share identical subtrees (common in incremental derivation), those subtrees should be detected and reused via `LoadableShardedMapV2Node` to avoid unnecessary enumeration and blob writes.

## Do NOT Flag

- Code that uses `from_entries_and_partial_maps` with a mix of `Either::Left` (individual entries) and `Either::Right` (reused partial maps) -- this is the correct pattern.
- Test code that constructs small sharded maps without optimization.
- WEIGHT_LIMIT values that have an accompanying justification comment.

## Recommendation

When implementing derived data types that use ShardedMapV2, compare parent subtrees at each trie level and reuse identical subtrees as `Either::Right(LoadableShardedMapV2Node)`. Only expand subtrees where parents disagree or where the PathTree has changes. Document WEIGHT_LIMIT choices with reasoning about expected entry sizes and access patterns.
