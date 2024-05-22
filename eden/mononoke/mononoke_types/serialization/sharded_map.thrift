/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! ------------
//! IMPORTANT!!!
//! ------------
//! Do not change the order of the fields! Changing the order of the fields
//! results in compatible but *not* identical serializations, so hashes will
//! change.
//! ------------
//! IMPORTANT!!!
//! ------------

include "eden/mononoke/mononoke_types/serialization/data.thrift"
include "eden/mononoke/mononoke_types/serialization/id.thrift"

/// Maximum size of a terminal node.
const i32 MAP_SHARD_SIZE = 2000;

// Since thrift has no "generics", we store the values of the map as arbitrary
// byte arrays. When parsing this, we will make sure they have the correct type.
// When non-trivial, the MapValue will be a Thrift serialized form of the true value.
typedef data.LargeBinary MapValue

// When the number of values in a subtree is at most MAP_SHARD_SIZE
// We inline all of them on a single node. Notice we don't use prefix here for
// simplicity.
struct ShardedMapTerminalNode {
  // The key is the original map key minus the prefixes and edges from all
  // intermediate nodes in the path to this node.
  1: map_SmallBinary_MapValue_6340 values;
} (rust.exhaustive)

// An intermediate node of the sharded map node tree, though it may have a
// value itself.
struct ShardedMapIntermediateNode {
  // Having this non-empty means this node was merged with its parents
  // since they had a single child only.
  1: data.SmallBinary prefix;
  // An intermediate node may have a single value.
  2: optional MapValue value;
  // Children of this node. We only store the first byte of the edge,
  // the remaining bytes are stored in the child node itself.
  3: map_byte_ShardedMapEdge_2565 edges;
} (rust.exhaustive)

// An edge from a map node to another
struct ShardedMapEdge {
  // Total count of values in this child's subtree, including the child.
  1: i64 size;
  // The actual child
  2: ShardedMapChild child;
} (rust.exhaustive)

// This represents either an inlined sharded map node, or an id of
// a node to be loaded from the blobstore.
union ShardedMapChild {
  1: ShardedMapNode inlined;
  2: id.ShardedMapNodeId id;
}

// A binary -> binary map that may be stored sharded in many different nodes.
//
// The final key/values of the map can be defined recursively as so:
// - If the map node is a terminal node, then we store all key/values directly
// within `values`.
// - If the map node is an intermediate node, then for each (byte, map) item of
// its `children` (where `map` may be stored inlined or as an id on the blobstore),
// prepend its keys with the single `byte` and then prepend them again with the `prefix`.
// If `value` is non-null, add a new key to the final map with key equal to `prefix`.
//
// For example, let's look at a concrete example, taking some liberties with notation:
// ShardedMapIntermediateNode {
//   prefix: "foo",
//   value: 12,
//   children: {
//     "b": ShardedMapTerminalNode {
//       values: {
//         "ar": 51,
//         "az": 69,
//       }
//     }
//   }
// }
//
// The "unsharded version" of this map is: {
//   "foo": 12,
//   "foobar": 51,
//   "foobaz": 69,
// }
//
// The representation of the sharded map doesn't make any assumptions about how the
// insertion/removal logic will actually shard the nodes, and any read operations
// should not as well, to ensure maximum compatibility with algorithm design changes.
union ShardedMapNode {
  1: ShardedMapIntermediateNode intermediate;
  2: ShardedMapTerminalNode terminal;
}

const i32 SHARDED_MAP_V2_WEIGHT_LIMIT = 2000;

typedef data.LargeBinary ShardedMapV2Value
typedef data.LargeBinary ShardedMapV2RollupData

struct ShardedMapV2StoredNode {
  1: id.ShardedMapV2NodeId id;
  2: i64 weight;
  3: i64 size;
  4: ShardedMapV2RollupData rollup_data;
} (rust.exhaustive)

union LoadableShardedMapV2Node {
  1: ShardedMapV2Node inlined;
  2: ShardedMapV2StoredNode stored;
}

// ShardedMapV2 is the same as ShardedMap except that it doesn't compress
// small subtrees into terminal nodes, instead it relies purely on inlining
// to solve the problem of having too many small blobs.
//
// Each ShardedMapV2Node has a conceptual weight which is defined as the sum of
// weights of all its inlined children, plus the count of its non-inlined children,
// plus one if it contains a value itself.
//
// To figure out which of a node's children are going to be inlined and which will
// not:
//    1) Recursively figure out inlining for each child's subtree.
//    2) Assume that all children are going to not be inlined and calculate the weight.
//    3) Iterate over children in order and for each check if inlining them will not make
//    the weight of the node go beyond SHARDED_MAP_V2_WEIGHT_LIMIT. If so inline them,
//    otherwise store them in a separate blob and store their id.
//
// This guarantees that the size of individual blobs will not grow too large, and
// should avoid creating too many small blobs in most cases. In particular, subtrees that
// would've have become a terminal node in ShardedMap will all be inlined in ShardedMapV2,
// with the added upside that they could potentially be stored inlined in their parent.
struct ShardedMapV2Node {
  1: data.SmallBinary prefix;
  2: optional ShardedMapV2Value value;
  3: map_byte_LoadableShardedMapV2Node_7012 children;
} (rust.exhaustive)

// The following were automatically generated and may benefit from renaming.
typedef map<data.SmallBinary, MapValue> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_SmallBinary_MapValue_6340
typedef map<byte, LoadableShardedMapV2Node> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_byte_LoadableShardedMapV2Node_7012
typedef map<byte, ShardedMapEdge> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_byte_ShardedMapEdge_2565
