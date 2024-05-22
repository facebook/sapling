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

include "eden/mononoke/mononoke_types/serialization/data.thrift"
include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/path.thrift"
include "eden/mononoke/mononoke_types/serialization/time.thrift"

// Parent ordering
// ---------------
// "Ordered" parents means that behavior will change if the order of parents
// changes.
// Whether parents are ordered varies by source control system.
// * In Mercurial, parents are stored ordered and the UI is order-dependent,
//   but are hashed unordered.
// * In Git, parents are stored and hashed ordered and the UI is also order-
//   dependent.
// These data structures will store parents in ordered form, as presented by
// Mercurial. This does hypothetically mean that a single Mercurial changeset
// can map to two Mononoke changesets -- those cases are extremely unlikely
// in practice, and if they're deliberately constructed Mononoke will probably
// end up rejecting whatever comes later.

// Other notes:
// * This uses sorted sets and maps to ensure deterministic
//   serialization.
// * Added and modified files are both part of file_changes.
// * file_changes is at the end of the struct so that a deserializer that just
//   wants to read metadata can stop early.
// * NonRootMPath, Id and DateTime fields do not have a reasonable default value, so
//   they must always be either "required" or "optional".
// * The set of keys in file_changes is path-conflict-free (pcf): no changed
//   path is a directory prefix of another path. So file_changes can never have
//   "foo" and "foo/bar" together, but "foo" and "foo1" are OK.
//   * If a directory is replaced by a file, the bonsai changeset will only
//     record the file being added. The directory being deleted is implicit.
//   * This only applies if the potential prefix is changed. Deleted files can
//     have conflicting subdirectory entries recorded for them.
//   * Corollary: The file list in Mercurial is not pcf, so the Bonsai diff is
//     computed separately.

struct BonsaiChangeset {
  1: list<id.ChangesetId> parents;
  2: string author;
  3: optional time.DateTime author_date;
  // Mercurial won't necessarily have a committer, so this is optional.
  4: optional string committer;
  5: optional time.DateTime committer_date;
  6: string message;
  // Extra headers specifically for mercurial
  7: map_string_binary_6626 hg_extra;
  // @lint-ignore THRIFTCHECKS bad-key-type
  8: map_NonRootMPath_FileChangeOpt_5342 file_changes;
  // Changeset is a snapshot iff this field is present
  9: optional SnapshotState snapshot_state;
  // Extra headers specifically for git. Both the key and the value
  // in these headers can be byte strings
  10: optional map_SmallBinary_LargeBinary_9715 git_extra_headers;
  // SHA1 hash representing a git tree object. If this changeset
  // corresponds to a Git tree object, then this field will have
  // value, otherwise it would be omitted.
  11: optional id.GitSha1 git_tree_hash;
  // Bonsai counterpart of git annotated tag. If the current changeset
  // represents an annotated tag, then this field will have a value.
  // Otherwise, it would be absent.
  12: optional BonsaiAnnotatedTag git_annotated_tag;
} (rust.exhaustive)

// Bonsai counterpart of a git annotated tag. This struct includes subset of
// tag properties. Rest can be represented using the fields in BonsaiChangeset.
// Specifically, the tag's name will be derived from the bookmark pointing to it.
// The tagger and message will be derived from the changeset author and message fields
// respectively.
// NOTE: This does not represent a lightweight tag, which is directly implemented as a
// bookmark in Mononoke.
struct BonsaiAnnotatedTag {
  1: BonsaiAnnotatedTagTarget target;
  2: optional data.LargeBinary pgp_signature;
} (rust.exhaustive)

// Target of an annotated tag imported from Git into Bonsai format.
union BonsaiAnnotatedTagTarget {
  1: id.ChangesetId Changeset; // Commmit, Tree or another Tag
  2: id.ContentId Content; // Blob
} (rust.exhaustive)

struct SnapshotState {
// Additional state for snapshots (if necessary)
} (rust.exhaustive)

enum FileType {
  Regular = 0,
  Executable = 1,
  Symlink = 2,
  GitSubmodule = 3,
}

struct FileChangeOpt {
  // All values being absent here means that the file was marked as deleted.
  // At most one value can be present.

  // Changes to a tracked file
  1: optional FileChange change;
  // This is a change to an untracked file in a snapshot commit.
  2: optional UntrackedFileChange untracked_change;
  // Present if this is a missing file in a snapshot commit.
  3: optional UntrackedDeletion untracked_deletion;
} (rust.exhaustive)

struct UntrackedDeletion {
// Additional state (if necessary)
} (rust.exhaustive)

struct UntrackedFileChange {
  1: id.ContentId content_id;
  2: FileType file_type;
  3: i64 size;
} (rust.exhaustive)

struct FileChange {
  1: id.ContentId content_id;
  2: FileType file_type;
  // size is a u64 stored as an i64
  3: i64 size;
  4: optional CopyInfo copy_from;
} (rust.exhaustive)

// This is only used optionally so it is OK to use `required` here.
struct CopyInfo {
  1: path.NonRootMPath file;
  // cs_id must match one of the parents specified in BonsaiChangeset
  2: id.ChangesetId cs_id;
} (rust.exhaustive)

// The following were automatically generated and may benefit from renaming.
typedef map<path.NonRootMPath, FileChangeOpt> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_NonRootMPath_FileChangeOpt_5342
typedef map<data.SmallBinary, data.LargeBinary> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_SmallBinary_LargeBinary_9715
typedef map<string, binary> (
  rust.type = "sorted_vector_map::SortedVectorMap",
) map_string_binary_6626
