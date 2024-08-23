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

struct SkeletonManifestDirectory {
  1: id.SkeletonManifestId id;
  2: SkeletonManifestSummary summary;
} (rust.exhaustive)

struct SkeletonManifestSummary {
  1: i64 child_files_count;
  2: i64 child_dirs_count;
  3: i64 descendant_files_count;
  4: i64 descendant_dirs_count;
  5: i32 max_path_len;
  6: i32 max_path_wchar_len;
  7: bool child_case_conflicts;
  8: bool descendant_case_conflicts;
  9: bool child_non_utf8_filenames;
  10: bool descendant_non_utf8_filenames;
  11: bool child_invalid_windows_filenames;
  12: bool descendant_invalid_windows_filenames;
} (rust.exhaustive)

struct SkeletonManifestEntry {
  // Present if this is a directory, absent for a file.
  1: optional SkeletonManifestDirectory directory;
} (rust.exhaustive)

// Structure-addressed manifest, with metadata useful for traversing manifest
// trees and determining case conflicts.
//
// Skeleton manifests form a manifest tree, where unique tree structure (i.e.
// the names of files and directories, but not their contents or history) is
// represented by a single skeleton manifest.  Skeleton manifest identities
// change when files are added or removed.
struct SkeletonManifest {
  1: map_MPathElement_SkeletonManifestEntry_4470 subentries;
  2: SkeletonManifestSummary summary;
} (rust.exhaustive)

// The following were automatically generated and may benefit from renaming.
typedef map<path.MPathElement, SkeletonManifestEntry> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_MPathElement_SkeletonManifestEntry_4470

// SkeletonManifestV2 is sharded version of SkeletonManifest that differs from it in two ways:
// 1) It doesn't include any extra metadata except a rollup count of its descendants that's used
// to enable ordered traversal. In particular this means that it can no longer be used for case
// conflicts checking which will be delegated to a different derived data type (`CaseConflictsManifest`).
// 2) It stores the sharded map inline without a layer of indirection, and relies only on the sharded
// map to decide which parts of the manifest should be inlined and which should be stored in a
// separate blob. This avoids the large number of tiny blobs that would otherwise be created
// for each small directory.
struct SkeletonManifestV2 {
  1: sharded_map.ShardedMapV2Node subentries;
} (rust.exhaustive)

struct SkeletonManifestV2File {} (rust.exhaustive)

union SkeletonManifestV2Entry {
  1: SkeletonManifestV2File file;
  2: SkeletonManifestV2 directory;
} (rust.exhaustive)
