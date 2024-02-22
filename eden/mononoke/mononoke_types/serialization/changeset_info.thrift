/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/data.thrift"
include "eden/mononoke/mononoke_types/serialization/time.thrift"

// Derived data structure that represents a Bonsai changeset's metadata.
// It contains the same data as Bonsai itself except of the file changes,
// which can be a pretty high number of and take a long time to deserialize.
//
// ChangesetInfo comes to resolve the necessity to waste time deserializing
// file changes, if there are many of them, when commit's metadata is the main
// reason the commit is being fetched.
struct ChangesetInfo {
  // Changeset id of the source Bonsai changeset
  1: id.ChangesetId changeset_id;
  2: list<id.ChangesetId> parents;
  3: string author;
  4: time.DateTime author_date;
  5: optional string committer;
  6: optional time.DateTime committer_date;
  7: ChangesetMessage message;
  8: HgExtras hg_extra;
  9: optional GitExtraHeaders git_extra_headers;
} (rust.exhaustive)

// Commit message is represented by a separate union of formats for the future
// flexibility reasons.
// At some point we may like to store large commit messages as separate blobs to
// make fetching changesets faster if there is no need in the whole description.
union ChangesetMessage {
  1: string message;
}

typedef map<string, binary> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) HgExtras

typedef map<data.SmallBinary, data.LargeBinary> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) GitExtraHeaders
