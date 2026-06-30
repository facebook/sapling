/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

namespace py3 eden.mononoke.mononoke_types.serialization

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

// Entry in a directory's (or file's) sharded map of subentries.
// Also used as parent pointers since the variants are the same.
union HistoryManifestEntry {
  1: id.HistoryManifestFileId file;
  2: id.HistoryManifestDirectoryId directory;
  3: id.HistoryManifestDeletedNodeId deleted_node;
}

// File node — tracks the history of a single file path.
// Each file modification or recreation produces a new node
// linked to its parents via HistoryManifestEntry.
// When a path was previously a directory, subentries carries
// the deleted history of all child paths.
@rust.Exhaustive
struct HistoryManifestFile {
  1: list<HistoryManifestEntry> parents;
  2: id.ContentId content_id;
  3: bonsai.FileType file_type;
  4: id.MPathHash path_hash;
  5: id.ChangesetId linknode;
  // Map of MPathElement -> HistoryManifestEntry
  // Present when this path was a directory at some point in history.
  6: sharded_map.ShardedMapV2Node subentries;
}

// Deleted node — tracks the deletion of a file or directory path.
// Unifies deleted-file and deleted-directory concepts. In merge commits
// where one parent has a path as a file and another as a directory,
// the deletion is represented uniformly by this single type.
@rust.Exhaustive
struct HistoryManifestDeletedNode {
  1: list<HistoryManifestEntry> parents;
  // Map of MPathElement -> HistoryManifestEntry
  // Contains the deleted history of child paths (if any existed).
  2: sharded_map.ShardedMapV2Node subentries;
  3: id.ChangesetId linknode;
}

// Directory node — tracks the history of a directory path.
// Subentries use ShardedMapV2 and may contain both live and deleted entries.
@rust.Exhaustive
struct HistoryManifestDirectory {
  1: list<HistoryManifestEntry> parents;
  // Map of MPathElement -> HistoryManifestEntry
  2: sharded_map.ShardedMapV2Node subentries;
  3: id.ChangesetId linknode;
}

struct HistoryManifestStageOutputEmpty {}

union HistoryManifestStageOutput {
  1: id.HistoryManifestFileId file_id;
  2: id.HistoryManifestDirectoryId directory_id;
  3: id.HistoryManifestDeletedNodeId deleted_node_id;
  4: HistoryManifestStageOutputEmpty empty;
}
