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
include "eden/mononoke/mononoke_types/serialization/bonsai.thrift"
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"
include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

namespace py3 eden.mononoke.mononoke_types.serialization

@rust.Exhaustive
struct ContentManifestFile {
  1: id.ContentId content_id;
  2: bonsai.FileType file_type;
  3: i64 size;
}

@rust.Exhaustive
struct ContentManifestDirectory {
  1: id.ContentManifestId id;
  2: ContentManifestRollupData rollup_data;
}

@rust.Exhaustive
struct ContentManifestCounts {
  1: i64 files_count;
  2: i64 dirs_count;
  3: i64 files_total_size;
}

// Composite rollup data stored in ShardedMapV2 shard nodes.
// Contains both "child" (flat/immediate) and "descendant" (recursive) counts
// so that both can be extracted in O(1) from the sharded map's rollup data
// without iterating all entries.
@rust.Exhaustive
struct ContentManifestRollupData {
  1: ContentManifestCounts child_counts;
  2: ContentManifestCounts descendant_counts;
}

union ContentManifestEntry {
  1: ContentManifestFile file;
  2: ContentManifestDirectory directory;
}

// Content-addressed manifest.
//
// Content manifests form a manifest tree where the type and contents of files
// and directories (but not their history) forms their identity.  Content
// manifest identities change when any file content is changed.
@rust.Exhaustive
struct ContentManifest {
  // Map of MPathElement -> ContentManifestEntry
  1: sharded_map.ShardedMapV2Node subentries;
}

struct ContentManifestStageOutputEmpty {}

union ContentManifestStageOutput {
  1: id.ContentManifestId content_manifest_id;
  2: ContentManifestStageOutputEmpty empty;
  3: ContentManifestFile content_manifest_file;
}
