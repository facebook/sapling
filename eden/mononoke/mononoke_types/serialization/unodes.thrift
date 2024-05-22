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

struct FileUnode {
  1: list<id.FileUnodeId> parents;
  2: id.ContentId content_id;
  3: bonsai.FileType file_type;
  4: id.MPathHash path_hash;
  5: id.ChangesetId linknode;
} (rust.exhaustive)

union UnodeEntry {
  1: id.FileUnodeId File;
  2: id.ManifestUnodeId Directory;
}

struct ManifestUnode {
  1: list<id.ManifestUnodeId> parents;
  2: map_MPathElement_UnodeEntry_3251 subentries;
  3: id.ChangesetId linknode;
} (rust.exhaustive)

// The following were automatically generated and may benefit from renaming.
typedef map<path.MPathElement, UnodeEntry> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_MPathElement_UnodeEntry_3251
