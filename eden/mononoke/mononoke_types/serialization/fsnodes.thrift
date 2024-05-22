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
include "eden/mononoke/mononoke_types/serialization/bonsai.thrift"

struct FsnodeFile {
  1: id.ContentId content_id;
  2: bonsai.FileType file_type;
  // size is a u64 stored as an i64
  3: i64 size;
  4: id.Sha1 content_sha1;
  5: id.Sha256 content_sha256;
} (rust.exhaustive)

struct FsnodeDirectory {
  1: id.FsnodeId id;
  2: FsnodeSummary summary;
} (rust.exhaustive)

struct FsnodeSummary {
  1: id.Sha1 simple_format_sha1;
  2: id.Sha256 simple_format_sha256;
  // Counts and sizes are u64s stored as i64s
  3: i64 child_files_count;
  4: i64 child_files_total_size;
  5: i64 child_dirs_count;
  6: i64 descendant_files_count;
  7: i64 descendant_files_total_size;
} (rust.exhaustive)

union FsnodeEntry {
  1: FsnodeFile File;
  2: FsnodeDirectory Directory;
}

// Content-addressed manifest, with metadata useful for filesystem
// implementations.
//
// Fsnodes form a manifest tree, where unique tree content (i.e. the names and
// contents of files and directories, but not their history) is represented by
// a single fsnode.  Fsnode identities change when any file content is changed.
//
// Fsnode metadata includes summary information about the content ID of
// files and manifests, and the number of files and sub-directories within
// directories.
struct Fsnode {
  1: map_MPathElement_FsnodeEntry_7103 subentries;
  2: FsnodeSummary summary;
} (rust.exhaustive)

// The following were automatically generated and may benefit from renaming.
typedef map<path.MPathElement, FsnodeEntry> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_MPathElement_FsnodeEntry_7103
