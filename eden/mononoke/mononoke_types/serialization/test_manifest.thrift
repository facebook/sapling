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

// TestManifest is a manifest type intended only to be used in tests. It contains
// only the file names and the maximum basename length of all files in each directory.
struct TestManifestFile {} (rust.exhaustive)
struct TestManifestDirectory {
  1: id.TestManifestId id;
  2: i64 max_basename_length;
} (rust.exhaustive)

union TestManifestEntry {
  1: TestManifestFile file;
  2: TestManifestDirectory directory;
} (rust.exhaustive)

struct TestManifest {
  1: map_MPathElement_TestManifestEntry_3039 subentries;
} (rust.exhaustive)

// TestShardedManifest is a sharded version of TestManifest (uses ShardedMapV2 in place of SortedVectorMap).
struct TestShardedManifestFile {
  // Storing the basename length of the file instead of calculating it from the edges from its parent
  // simplifies the derivation logic.
  1: i64 basename_length;
} (rust.exhaustive)
struct TestShardedManifestDirectory {
  1: id.TestShardedManifestId id;
  2: i64 max_basename_length;
} (rust.exhaustive)

union TestShardedManifestEntry {
  1: TestShardedManifestFile file;
  2: TestShardedManifestDirectory directory;
} (rust.exhaustive)

struct TestShardedManifest {
  1: sharded_map.ShardedMapV2Node subentries;
} (rust.exhaustive)

// The following were automatically generated and may benefit from renaming.
typedef map<path.MPathElement, TestManifestEntry> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_MPathElement_TestManifestEntry_3039
