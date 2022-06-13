# Mononoke sharded maps

This document has the description of how sharded maps work in Mononoke. Sharded maps are a way to store mappings that can be sharded over multiple blobs in the blobstore. In this way, it's not necessary to load the whole map from the backend if we just to access a few values.

See [the original doc](https://fb.quip.com/ktMoAYnNwbUu) for the initial draft, with more context, a problem statement, and details as to why this decision was taken. The current doc details only the final design/implementation of sharded maps, which is based on the initial suggestion with a few added optimisations/modifications.

A sharded map is stored as a trie, with the following optimisations:

1. Edges are compressed when a node has a single child. ([Optimisation #1](https://fb.quip.com/ktMoAYnNwbUu#temp:C:ZHD3882f8162eebf12ceec638ca3) in the original doc).
2. If the whole subtree of a node is small enough (at most `K = 2000` values), all its key/values are stored inline in a terminal node ([Optimisation #2](https://fb.quip.com/ktMoAYnNwbUu#temp:C:ZHD19d31ebd630214c528c5ba660) in the original doc).
3. Only terminal nodes (previous optimisation) are stored in separate blobstore blobs, all intermediate nodes are stored inline in a single blob (this merges [Simon's suggestion](https://fb.quip.com/ktMoAYnNwbUu#ZHDADAusxB8) to store some blobs inline with Mateusz's suggestion to store all intermediate nodes inline, which led to [this analysis](https://fb.quip.com/rylJAj2zeayH)).

## Representation

```
enum ShardedMapNode<V> {
    Intermediate {
        prefix: Vec<u8>,
        value: Option<V>,
        value_count: usize,
        children: Map<u8, MapIdOrInlined<V>>,
    },
    Terminal {
        values: Map<Vec<u8>, V>,
    },
}

enum MapIdOrInlined<V> {
    Id(ShardedMapNodeId),
    Inlined(ShardedMapNode<V>),
}
```

This is a simplified representation of the sharded maps. It has all the same fields as the actual Rust implementation, but skips over details such as using `SmallVec` for arrays and `SortedVectorMap` for maps, which are trivial implementation details.

The thrift definition is analogous, but notice that there's no generics in thrift, so all values are stored as binary, instead of a generic `V`. In Rust we deserialise these values when reading and fail if they're invalid.

We do optimisation 1 by storing the `prefix` field in Intermediate nodes, which compresses all parents of the node that had a single child. Optimisation 2 is done by using the Terminal nodes. Optimisation 3 is enabled by using `MapIdOrInlined`, which may store an inlined node or a blobstore id for the node.

### Recursive definition

The map is a recursive structure (it has submaps inside of it), so the easiest way to explain how the data is stored is recursively.

* On a terminal node, all key-values are stored directly in `values`, with no modifications or optimisations.
* On an intermediate node, for each `(byte, submap)`, prepend all keys of `submap` with byte, then prepend all keys in all submaps with `prefix`. If `value` is present, add the `(prefix, value)` key-value pair to the final map.

## Read operations

Notice that the representation itself does not make the distinction that a node is inlined iff it's an intermediate node, or that terminal nodes have at most `K` values, which makes it easier to change these optimisations in the future and keep read compatibility. Read operations should work independently of how sharded maps are stored (splitting and inlining strategies).

### lookup

Given a key, what's the value for that key, if any? Equivalent of `get` on regular maps.

Simplified API:

```
fn lookup(&self, key: &[u8]) → Option<V>;
```

Logic:

* (Case 1) If the node is a terminal node, then just do the lookup directly on the inlined map.
* If the node is an intermediate node, there are a few sub-cases:
    * If `prefix` is a prefix of `key`:
        * If `len(key) > len(prefix)`, consider `b = key[len(prefix)]`, that is, the byte right after the prefix
            * (Case 2) If `children` has key `b`, do a recursive `lookup(&key[len(prefix)+1..])` call on the associated submap, first loading it from blobstore if it was not inlined, and removing `prefix` and `b` from `key`.
            * (Case 3) If `children` does not have the key `b`, then `key` is not present in the map, return `None`.
        * (Case 4) Else then `key = prefix`, then the node for this key is the current intermediate node, so return value.
    * (Case 5) If `prefix` is not a prefix of `key`, then `key` is not present in the map, return `None`.

### into_entries

Iterates through all values in the map, asynchronously and only loading blobs as needed. Equivalent of `into_iter` on regular maps.

Simplified API:

```
fn into_entries(self) -> impl Stream<Result<Vec<u8>, V>>;
```

Logic:
The `into_entries` method works by doing a depth first search on the trie (which in the end is just a modified tree), while outputting the final key-value pairs. The state carried through the DFS is `cur_prefix: Vec<u8>`, the accumulated prefix of all nodes up to but not including the current node. The DFS logic can be recursively explained as such:

* (Case 1) If the node is a terminal node, prepend all the keys in the `values` map with `cur_prefix`, and output those key-value pairs.
* (Case 2) If the node is an intermediate node, do these following steps:

    1. Extend `cur_prefix` with this node's `prefix` field.
    2. If `value` is present, output `(cur_prefix, value)`, as a key-value pair.
    3. For each of `(byte, submap)` in `children`, create a copy of `cur_prefix`, add `byte` as the last byte, and recurse into the DFS for `submap`.

## Write operations

We can generalise all write operations under a single `update` operation that receives a map of key-values to add or remove from the current map.

The reason for merging all operations under this single operation is that it is faster than doing the operations separately, and it also avoids adding "temporary" blobs to the blobstore that are never being accessed again.

The write operations need to be careful about maintaining the properties of the optimisations (e.g. terminal nodes with at most `K` values, inlining all intermediate nodes), but it assumes the current structure already has these properties. If it does not, the write operations should still produce a "valid" data structure, though it might not uphold the properties.

### update

Create a new map from this map with given replacements.

Simplified API:

```
fn update(replacements: Map<Vec<u8>, Option<V>>) -> ShardedMapNode<V>;
```

The operation does **not** assume that the replacements apply "cleanly", that is, there might be keys that are replaced, or keys to remove that do not exist. If we change this assumption in the future we need to modify all use points to comply with it.

Logic:

* If on a terminal node, apply `replacements` to the current `values`:
    * (Case 1) If the updated `values` has at most `K` elements, return a terminal node with those values.
    * (Case 2) Otherwise, this will be an intermediate node. You can recursively call the update operation on an empty intermediate node using the updated `values` as `replacements`. (TODO: assumptions? prefix?)
* If on an intermediate node, calculate the LCP (longest common prefix) of `prefix` and all the keys in `replacements` that are additions.
    * (Case 3) If LCP is smaller than `prefix`, this means we need to split the current node in two.
        * We need to split the prefix in 3 like IMAGE 2 below:
            * left: This is the LCP of all replacements
            * mid: The byte after the LCP. Since `len(LCP)<len(prefix)`, this always exists.
            * right: The remaining part of the prefix, may be empty.
        * With that, we split the current node into two nodes, like in IMAGE 2 below:
            * `left_node` has the first part of the prefix, which is LCP
            * `right_node` has the remaining part of the prefix, and all the same children as the current node.
        * After the split, we recursively call `update` for `left_node` with the same `replacements`, and now we have transformed Case 3 into Case 4.
    * (Case 4) If LCP is `prefix`, all addition keys have `prefix`.
        * (Step 4.1) Strip the prefix for all the keys (and ignore deletion keys that don't have `prefix`, it simply means they do not exist), and partition them according to their next byte. If they don't have a next byte, it means their value is the value of the current intermediate node, so update that.
        * (Step 4.2) For the partitioned replacements, recursively call `update` to update the correspondent child of the intermediate node. If it doesn't have such child, create an empty terminal node to be updated.
        * Finishing up:
            * (Case 4.3.1) If the updated node has at most K total values after this, compress it to a terminal node
            * (Case 4.3.2) Otherwise, for the new updated children, inline the intermediate nodes and store the terminal nodes in the blobstore (according to optimisation 3).

                After this, `children` will always have at least one element (otherwise case 4.3.1 would've happened).

                If `children` has exactly one element and `value` is none, compress the node with its single child.
                Otherwise, return the modified intermediate node directly.

* * *

IMAGE 1
```
      prefix
/----   -  -----\
 left  mid right
 (lcp)
```

IMAGE 2
```
BEFORE:              AFTER
o                    o
 \                    \ left
  \ prefix   ==>       o left_node
   \                    \ mid + right
    o                    o
  cur_node             right_node
```
