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

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/path.thrift"
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"

struct DeletedManifest {
  1: optional id.ChangesetId linknode;
  2: map_MPathElement_DeletedManifestId_4196 subentries;
} (rust.exhaustive)

struct DeletedManifestV2 {
  1: optional id.ChangesetId linknode;
  // Map of MPathElement -> DeletedManifestV2Id
  2: sharded_map.ShardedMapNode subentries;
} (rust.exhaustive)

// The following were automatically generated and may benefit from renaming.
typedef map<path.MPathElement, id.DeletedManifestId> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_MPathElement_DeletedManifestId_4196
