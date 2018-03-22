// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! ------------
//! IMPORTANT!!!
//! ------------
//! Do not change the order of the fields! Changing the order of the fields
//! results in compatible but *not* identical serialiations, so hashes will
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

typedef Blake2 UnodeId (hs.newtype)
typedef Blake2 ChangesetId (hs.newtype)
typedef Blake2 ContentId (hs.newtype)

// A path in a repo is stored as a list of elements. This is so that the sort
// order of paths is the same as that of a tree traversal, so that deltas on
// manifests can be applied in a streaming way.
typedef binary MPathElement (hs.newtype)
typedef list<MPathElement> MPath (hs.newtype)

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
// * file_changes and file_deletes are at the end of the struct so that a
//   deserializer that just wants to read metadata can stop early.
struct TinyChangeset {
  1: required list<ChangesetId> parents,
  2: required string user,
  3: required DateTime date,
  4: required string message,
  5: required map<string, string> extra,
  6: required map<MPath, FileChange> file_changes,
  7: required set<MPath> file_deletes,
}

struct DateTime {
  1: required i64 timestamp_secs,
  // Timezones can go up to UTC+13 (which would be represented as -46800), so
  // an i16 can't fit them.
  2: required i32 tz_offset_secs,
}

union FileContents {
  1: binary Bytes,
}

enum FileType {
  Regular = 0,
  Executable = 1,
  Symlink = 2,
}

struct FileChange {
  1: required ContentId content_id,
  2: required FileType file_type,
  // size is a u64 stored as an i64
  3: required i64 size,
  4: optional MPath copy_from,
}
