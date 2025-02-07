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
include "thrift/annotation/rust.thrift"

// TestManifest is a manifest type intended only to be used in tests. It contains
// only the file names and the maximum basename length of all files in each directory.
@rust.Exhaustive
struct TestManifestFile {}
@rust.Exhaustive
struct TestManifestDirectory {
  1: id.TestManifestId id;
  2: i64 max_basename_length;
}

union TestManifestEntry {
  1: TestManifestFile file;
  2: TestManifestDirectory directory;
}

@rust.Exhaustive
struct TestManifest {
  1: map_MPathElement_TestManifestEntry_3039 subentries;
}

// TestShardedManifest is a sharded version of TestManifest (uses ShardedMapV2 in place of SortedVectorMap).
@rust.Exhaustive
struct TestShardedManifestFile {
  // Storing the basename length of the file instead of calculating it from the edges from its parent
  // simplifies the derivation logic.
  1: i64 basename_length;
}
@rust.Exhaustive
struct TestShardedManifestDirectory {
  1: id.TestShardedManifestId id;
  2: i64 max_basename_length;
}

union TestShardedManifestEntry {
  1: TestShardedManifestFile file;
  2: TestShardedManifestDirectory directory;
}

@rust.Exhaustive
struct TestShardedManifest {
  1: sharded_map.ShardedMapV2Node subentries;
}

// The following were automatically generated and may benefit from renaming.
@rust.Type{name = "sorted_vector_map::SortedVectorMap"}
typedef map<
  path.MPathElement,
  TestManifestEntry
> map_MPathElement_TestManifestEntry_3039
