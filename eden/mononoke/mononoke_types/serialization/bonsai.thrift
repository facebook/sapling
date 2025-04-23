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
include "thrift/annotation/rust.thrift"

// Bonsai Changeset is the fundamental commit object of Mononoke's internal
// representation.
//
// * Parents are stored and hashed ordered.  This matches Git, but differs
//   from Mercurial/Sapling where parents are stored ordered but hashed
//   unordered.  This means that a single Mercurial/Sapling changeset can
//   map to two Mononoke changesets, but this is extremely unlikely in
//   practice and Mononoke will reject whichever order comes later.
// * Sorted sets and maps are used to ensure deterministic serialization.
// * There is no distinction between added and modified files in file_changes.
// * Path conflicts in file_changes have the following meanings:
//   - A deleted path may be a prefix of changed paths and means a file was
//     replaced by a directory.
// * Otherwise, path conflicts are not allowed (a change cannot be a prefix
//   of a deletion or another change)
// * Subtree changes are permitted to be the same as or a prefix of a file
//   change.  The subtree change happens first, and then file changes are
//   applied on top.

@rust.Exhaustive
struct BonsaiChangeset {
  1: list<id.ChangesetId> parents;
  2: string author;
  3: optional time.DateTime author_date;
  // Mercurial won't necessarily have a committer, so this is optional.
  4: optional string committer;
  5: optional time.DateTime committer_date;
  6: string message;
  // Extra headers specifically for mercurial
  7: HgExtras hg_extra;
  // @lint-ignore THRIFTCHECKS bad-key-type
  8: FileChanges file_changes;
  // Changeset is a snapshot iff this field is present
  9: optional SnapshotState snapshot_state;
  // Extra headers specifically for git. Both the key and the value
  // in these headers can be byte strings
  10: optional GitExtraHeaders git_extra_headers;
  // SHA1 hash representing a git tree object. If this changeset
  // corresponds to a Git tree object, then this field will have
  // value, otherwise it would be omitted.
  11: optional id.GitSha1 git_tree_hash;
  // Bonsai counterpart of git annotated tag. If the current changeset
  // represents an annotated tag, then this field will have a value.
  // Otherwise, it would be absent.
  12: optional BonsaiAnnotatedTag git_annotated_tag;
  // Changes (copies or merges) that apply to entire subtrees.
  // @lint-ignore THRIFTCHECKS bad-key-type
  13: optional SubtreeChanges subtree_changes;
}

// Bonsai counterpart of a git annotated tag. This struct includes subset of
// tag properties. Rest can be represented using the fields in BonsaiChangeset.
// Specifically, the tag's name will be derived from the bookmark pointing to it.
// The tagger and message will be derived from the changeset author and message fields
// respectively.
// NOTE: This does not represent a lightweight tag, which is directly implemented as a
// bookmark in Mononoke.
@rust.Exhaustive
struct BonsaiAnnotatedTag {
  1: BonsaiAnnotatedTagTarget target;
  2: optional data.LargeBinary pgp_signature;
}

// Target of an annotated tag imported from Git into Bonsai format.
union BonsaiAnnotatedTagTarget {
  1: id.ChangesetId Changeset; // Commit, Tree or another Tag
  2: id.ContentId Content; // Blob
}

@rust.Exhaustive
struct SnapshotState {
// Additional state for snapshots (if necessary)
}

enum FileType {
  Regular = 0,
  Executable = 1,
  Symlink = 2,
  GitSubmodule = 3,
}

@rust.Type{name = "sorted_vector_map::SortedVectorMap"}
typedef map<path.NonRootMPath, FileChangeOpt> FileChanges

@rust.Exhaustive
struct FileChangeOpt {
  // All values being absent here means that the file was marked as deleted.
  // At most one value can be present.

  // This is a change to a tracked file.
  1: optional FileChange change;
  // This is a change to an untracked file in a snapshot commit.
  2: optional UntrackedFileChange untracked_change;
  // This is a missing file in a snapshot commit.
  3: optional UntrackedDeletion untracked_deletion;
}

@rust.Exhaustive
struct FileChange {
  1: id.ContentId content_id;
  2: FileType file_type;
  // size is a u64 stored as an i64
  3: i64 size;
  4: optional CopyInfo copy_from;
  // This structure present means this file should be represented
  // as Git LFS pointer when served via Git data formats.
  5: optional GitLfs git_lfs;
}

@rust.Exhaustive
struct UntrackedFileChange {
  1: id.ContentId content_id;
  2: FileType file_type;
  3: i64 size;
}

@rust.Exhaustive
struct UntrackedDeletion {
// Additional state (if necessary)
}

@rust.Exhaustive
struct CopyInfo {
  1: path.NonRootMPath file;
  // cs_id must match one of the parents specified in BonsaiChangeset
  2: id.ChangesetId cs_id;
}

@rust.Type{name = "sorted_vector_map::SortedVectorMap"}
typedef map<string, binary> HgExtras

@rust.Type{name = "sorted_vector_map::SortedVectorMap"}
typedef map<data.SmallBinary, data.LargeBinary> GitExtraHeaders

// Git LFS
// Just mere presence of this structure is enough to get the file changes
// represented as Git LFS pointer when served using Git data formats.
//
// Leaving this datastructure entirely empty is recommended when creating new
// commits originating from outside of Git. But if the commit was created by
// by rougue client and the pointer is not exactly byte-for-byte equal to what
// Mononoke would create then data here is used to ensure data rountripability.
//
// by canonical pointer we mean one like:
// version https://git-lfs.github.com/spec/v1\noid sha256:{sha256}\nsize {size}\n
//
// see: https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md
struct GitLfs {
  1: optional id.ContentId non_canonical_pointer_content_id;
  // If there's any version of Git LFS format beyond v1 then we should
  // have an enum here to indicate the version number. Right now there's just
  // one version: v1.
}

// A subtree copy marks an entire subtree as *copied* from a path in a
// different commit.  History operations for these files should divert
// down the history of the source commit.
//
// The copy is a "shallow" copy, which means the manifest from the copy
// source should be reused, and only additional changes are listed in
// the file_changes.
struct SubtreeCopy {
  // Path in the source commit the subtree is copied form.
  1: path.MPath from_path;

  // Source commit.  This may or may not be an ancestor.
  2: id.ChangesetId from_cs_id;
}

// A subtree deep copy marks an entire subtree as *copied* from a path in a
// different commit.  History operations for these files should divert
// down the history of the source commit.
//
// The copy is a "deep" copy, which means the file changes in the bonsai
// changeset include all changes necessary to perform the copy, plus any
// additional changes on top.
struct SubtreeDeepCopy {
  // Path in the source commit the subtree is copied form.
  1: path.MPath from_path;

  // Source commit.  This may or may not be an ancestor.
  2: id.ChangesetId from_cs_id;
}

// A subtree merge marks an entire subtree as *merge* with another path,
// possibly in a different commit.  History operations for these files
// include merging the history of the source commit.
//
// Subtree merges are always "deep", which means the file changes in the
// bonsai changeset include all changes necessary to perform the merge.
struct SubtreeMerge {
  // Path in the source commit the subtree is copied form.
  1: path.MPath from_path;

  // Source commit.  This may or may not be an ancestor.
  2: id.ChangesetId from_cs_id;
}

// A subtree import marks an entire subtree as *imported* from a path in a
// different repository.  History operations for these files should yield
// the link to the source repository.   Note that Mononoke does not follow
// these links, it merely stores the fact that the link exists.
struct SubtreeImport {
  // Path in the source commit the subtree is copied form.
  1: path.MPath from_path;

  // Source commit id in the source repository.  The exact meaning of this
  // id is up to the source repository.
  2: string from_commit;

  // Source repository url.
  3: string from_repo_url;
}

// A change that applies to a whole subtree.
//
// Subtree changes can have two kinds of effects:
//
//   - "History modifying" means that history for the subtree (the directory and
//     all files and directories inside it) is in some way altered or diverted.
//
//   - "Manifest altering" means the effective manifest (i.e. what files and trees
//     are present) of the repository is altered by the presence of the subtree
//     copy.  Manifest alterations should be applied *before* the file changes.
union SubtreeChange {
  // This subtree is copied from another location in the repository.
  //
  // This change is history modifying: history should be diverted to the copy source.
  //
  // This change is manifest altering: the source subtree should be reused instead
  // of any subtree that is already there.  That is, this copy implicitly deletes
  // all files at the destination and replaces them with all files located at the
  // source.
  1: SubtreeCopy subtree_copy;
  // This subtree is copied from another location in the repository.
  //
  // This change is history modifying: history should be diverted to the copy source.
  //
  // This change is *not* manifest altering.  The file changes will contain the
  // necessary changes to replace the copy destination with the contents of the source.
  2: SubtreeDeepCopy subtree_deep_copy;
  // This subtree is merged with another location in the repository.
  //
  // This change is history modifying: history should be merged with the copy source.
  //
  // This change is *not* manifest altering.   The file changes will contain the
  // necessary changes to merge the source with the destination.
  3: SubtreeMerge subtree_merge;
  // This subtree is imported from another repository.
  //
  // This change modifies history to point to another repository.  Note: Mononoke
  // does not follow that link, it merely stores the fact that the link exists.
  //
  // This change is *not* manifest altering.  The file changes will contain the
  // necessary changes to import the source to the destination.
  4: SubtreeImport subtree_import;
}

@rust.Type{name = "sorted_vector_map::SortedVectorMap"}
typedef map<path.MPath, SubtreeChange> SubtreeChanges
