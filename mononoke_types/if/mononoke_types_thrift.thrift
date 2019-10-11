/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
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

// Thrift doesn't have fixed-length arrays, so a 256-bit hash can be
// represented in one of two ways:
// 1. as four i64s
// 2. as just a newtype around a `binary`
//
// Representation 1 is very appealing as it provides a 1:1 map between Rust's
// data structures and Thrift's. But it means that the full hash is not
// available as a single contiguous block in memory. That makes some
// zero-copy optimizations hard.
// Representation 2 does have the benefit of the hash being available as a
// contiguous block, but it requires runtime length checks. With the default
// Rust representation it would also cause a heap allocation.
// Going with representation 2, with the hope that this will be able to use
// SmallVecs soon.
// TODO (T26959816): add support to represent these as SmallVecs.
typedef binary Blake2 (hs.newtype)

// Allow the hash type to change in the future.
union IdType {
  1: Blake2 Blake2,
}

typedef IdType ChangesetId (hs.newtype)
typedef IdType ContentId (hs.newtype)
typedef IdType ContentChunkId (hs.newtype)
typedef IdType RawBundle2Id (hs.newtype)
typedef IdType FileUnodeId (hs.newtype)
typedef IdType ManifestUnodeId (hs.newtype)
typedef IdType FsnodeId (hs.newtype)
typedef IdType MPathHash (hs.newtype)

typedef IdType ContentMetadataId (hs.newtype)
typedef IdType FastlogBatchId (hs.newtype)


// mercurial_types defines Sha1, and it's most convenient to stick this in here.
// This can be moved away in the future if necessary. Could also be used for
// raw content sha1 (should this be separated?)
typedef binary Sha1 (hs.newtype)

// Other content alias types
typedef binary Sha256 (hs.newtype)
typedef binary GitSha1 (hs.newtype)

// A path in a repo is stored as a list of elements. This is so that the sort
// order of paths is the same as that of a tree traversal, so that deltas on
// manifests can be applied in a streaming way.
typedef binary MPathElement (hs.newtype)
typedef list<MPathElement> MPath (hs.newtype)

union RepoPath {
  # Thrift language doesn't support void here, so put a dummy bool
  1: bool RootPath,
  2: MPath DirectoryPath,
  3: MPath FilePath,
}

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
// * This uses sorted (B-tree) sets and maps to ensure deterministic
//   serialization.
// * Added and modified files are both part of file_changes.
// * file_changes is at the end of the struct so that a deserializer that just
//   wants to read metadata can stop early.
// * The "required" fields are only for data that is absolutely core to the
//   model. Note that Thrift does allow changing "required" to unqualified.
// * MPath, Id and DateTime fields do not have a reasonable default value, so
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
  1: required list<ChangesetId> parents,
  2: string author,
  3: optional DateTime author_date,
  // Mercurial won't necessarily have a committer, so this is optional.
  4: optional string committer,
  5: optional DateTime committer_date,
  6: string message,
  7: map<string, binary> extra,
  8: map<MPath, FileChangeOpt> file_changes,
}

// DateTime fields do not have a reasonable default value! They must
// always be required or optional.
struct DateTime {
  1: required i64 timestamp_secs,
  // Timezones can go up to UTC+13 (which would be represented as -46800), so
  // an i16 can't fit them.
  2: required i32 tz_offset_secs,
}

struct ContentChunkPointer {
  1: ContentChunkId chunk_id,
  2: i64 size,
}

// When a file is chunked, we reprsent it as a list of its chunks, as well as
// its ContentId.
struct ChunkedFileContents {
  // The ContentId is here to ensure we can reproduce the ContentId from the
  // FileContents reprseentation in Mononoke, which would normally require
  // hashing the contents (but we obviously can't do that here, since we don't
  // have the contents).
  1: ContentId content_id,
  2: list<ContentChunkPointer> chunks,
}

union FileContents {
   // Plain uncompressed bytes - WYSIWYG.
  1: binary Bytes,
  // References to Chunks (stored as FileContents, too).
  2: ChunkedFileContents Chunked,
}

union ContentChunk {
  1: binary Bytes,
}

// Payload of object which is an alias
union ContentAlias {
  1: ContentId ContentId, // File content alias
}

// Metadata about a file. This includes hahs aliases, or the file's size.
// NOTE: Fields 1 through 5 have always been written by Mononoke, and Mononoke
// expects them to be present when reading ContentMetadata structs back
// from its Filestore. They're marked optional so we can report errors if
// they're absent at runtime (as opposed to letting Thrift give us a default
// value).
struct ContentMetadata {
  // total_size is needed to make GitSha1 meaningful, but generally useful
  1: optional i64 total_size,
  // ContentId we're providing metadata for
  2: optional ContentId content_id,
  3: optional Sha1 sha1,
  4: optional Sha256 sha256,
  // always object type "blob"
  5: optional GitSha1 git_sha1,
}

union RawBundle2 {
  1: binary Bytes,
}

enum FileType {
  Regular = 0,
  Executable = 1,
  Symlink = 2,
}

struct FileChangeOpt {
  // The value being absent here means that the file was deleted.
  1: optional FileChange change,
}

struct FileChange {
  1: required ContentId content_id,
  2: FileType file_type,
  // size is a u64 stored as an i64
  3: required i64 size,
  4: optional CopyInfo copy_from,
}

// This is only used optionally so it is OK to use `required` here.
struct CopyInfo {
  1: required MPath file,
  // cs_id must match one of the parents specified in BonsaiChangeset
  2: required ChangesetId cs_id,
}

struct FileUnode {
  1: list<FileUnodeId> parents,
  2: ContentId content_id,
  3: FileType file_type,
  4: MPathHash path_hash,
  5: ChangesetId linknode,
}

union UnodeEntry {
  1: FileUnodeId File,
  2: ManifestUnodeId Directory,
}

struct ManifestUnode {
  1: list<ManifestUnodeId> parents,
  2: map<MPathElement, UnodeEntry> subentries,
  3: ChangesetId linknode,
}

struct FsnodeFile {
  1: ContentId content_id,
  2: FileType file_type,
  // size is a u64 stored as an i64
  3: i64 size,
  4: Sha1 content_sha1,
  5: Sha256 content_sha256,
}

struct FsnodeDirectory {
  1: FsnodeId id,
  2: FsnodeSummary summary,
}

struct FsnodeSummary {
  1: Sha1 simple_format_sha1,
  2: Sha256 simple_format_sha256,
  // Counts and sizes are u64s stored as i64s
  3: i64 child_files_count,
  4: i64 child_files_total_size,
  5: i64 child_dirs_count,
  6: i64 descendant_files_count,
  7: i64 descendant_files_total_size,
}

union FsnodeEntry {
  1: FsnodeFile File,
  2: FsnodeDirectory Directory,
}

struct Fsnode {
  1: map<MPathElement, FsnodeEntry> subentries,
  2: FsnodeSummary summary,
}

// Structure that holds a commit graph, usually a history of a file
// or a directory hence the name. Semantically it stores list of
// (commit hash, [parent commit hashes]), however it's stored in compressed form
// described below. Compressed form is used to save space.
//
// FastlogBatch has two parts: `latest` and `previous_batches`.
// `previous_batches` field points to another FastlogBatch structures so
// FastlogBatch is a recursive structure. However normally `previous_batches`
// point to degenerate version of FastlogBatch with empty `previous_batches`
// i.e. we have only one level of nesting.
//
// In order to get the full list we need to get latest commits and concatenate
// it with lists from `previous_batches`.
//
// `latest` stores commit hashes and offsets to commit parents
// i.e. if offset is 1, then next commit is a parent of a current commit.
// For example, a list like
//
//  (HASH_A, [HASH_B])
//  (HASH_B, [])
//
//  will be encoded as
//  (HASH_A, [1])  # offset is 1, means next hash
//  (HASH_B, [])
//
//  A list with a merge
//  (HASH_A, [HASH_B, HASH_C])
//  (HASH_B, [])
//  (HASH_C, [])
//
//  will be encoded differently
//  (HASH_A, [1, 2])
//  (HASH_B, [])
//  (HASH_C, [])
//
// Note that offset might point to a commit in a next FastlogBatch or even
// point to batch outside of all previous_batches.
struct FastlogBatch {
  1: list<CompressedHashAndParents> latest,
  2: list<FastlogBatchId> previous_batches,
}

typedef i32 ParentOffset (hs.newtype)

struct CompressedHashAndParents {
  1: ChangesetId cs_id,
  # Offsets can be negative!
  2: list<ParentOffset> parent_offsets,
}
