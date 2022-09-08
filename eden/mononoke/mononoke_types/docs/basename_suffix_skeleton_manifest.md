# Basename suffix skeleton manifest

This document has the description of how basename suffix skeleton manifest (BSSM) work in Mononoke.

## Context

Manifests are derived data tree structures that store information about the repository in Mononoke.

Common queries to have in Mononoke (and we have an SCS endpoint that answers it inefficiently) are:
- List all TARGETS in the repo.
- List all py files under this directory.

The current implementation for the queries above lists all files in the directory, and then filters it by basename or extension, and so is very inefficient.

## Structure

BSSM works like skeleton manifest, but it modifies the path before creating the manifest. Essentially, it stores the skeleton manifest for a modified repo.

To modify the path we copy the basename as the first directory, and reverse it. So `fbcode/eden/mononoke/TARGETS` becomes `STEGRAT/fbcode/eden/mononoke/TARGETS`.

Now, a query to list files of the pattern `fbcode/eden/**/TARGETS` becomes a query to list files of the pattern `STEGRAT/fbcode/eden/**`. It is possible to answer this query efficiently, and we already have code to do so (efficient = `~O(results)`).

### FAQ

**Why reverse the basename?**

This way queries like `fbcode/eden/**/*.py` can be rewritten as `yp.*/fbcode/eden/**`. Our data structures (see `sharded_map.md`) allow for efficient querying based on prefix, which helps optimise queries like this. This is still not implemented, though the design supports it.

**Why keep the basename in the end as well?**

The reason is two-fold:
- Keeping a filename in the end prevents directories having the same name as files, as in the example `a/b/c -> c/a/b` and `a/c -> c/a`.
- Using the basename, and not a sentinel like `$`, makes the relative ordering of files remain the same, which is useful for ordered queries.

**Is this just a heuristic?**

Yes and no. It does makes queries based on basenames as efficient as they could be, but it doesn't help completely fix queries like `fbcode/eden/**/src/main.rs` because the suffix is not just a basename. As far as we are aware, there is no efficient way to solve all queries with any suffix and any prefix, so BSSM helps with basenames only, which is the most common query.

**Can this also count files efficiently? (Not just list them)**

Not yet. By adding some associated data, we could count all `TARGETS` files under a directory, but it will need some more complicated work to also be able to count all `*.py` files efficiently. The aggregated data would need to be piped *through* the sharded maps, which is possible but needs extra work on sharded maps.

To count other things, like total line count, another derived data would need to be implemented. This is like skeleton manifest, and thus *doesn't depend on any file contents*. An equivalent of this for fsnodes would need to be implemented.

**Won't there be too many directories top-level??**

Nice catch, yes there will. Every basename in the repo will become a top-level directory, which means millions of top-level directories. That is why we're using sharded maps for this data structure, as it should be able to cope with really large maps without wasting storage or time.
