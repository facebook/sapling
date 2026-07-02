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
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"
include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

namespace py3 eden.mononoke.mononoke_types.serialization

@rust.Exhaustive
struct DeletedManifestV2 {
  1: optional id.ChangesetId linknode;
  // Map of MPathElement -> DeletedManifestV2Id
  2: sharded_map.ShardedMapNode subentries;
}

struct DeletedManifestStageOutputEmpty {}

// Per-stage output for pipeline (multi-stage) derivation of DeletedManifestV2.
// A deleted manifest node is homogeneous, so a stage subtree is a single
// optional node id (`empty` means no deletions under the stage path).
union DeletedManifestStageOutput {
  1: id.DeletedManifestV2Id deleted_manifest_id;
  2: DeletedManifestStageOutputEmpty empty;
}
