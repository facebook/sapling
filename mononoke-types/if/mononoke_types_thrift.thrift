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

typedef Blake2 UnodeHash (hs.newtype)
typedef Blake2 ChangesetId (hs.newtype)
typedef Blake2 BlobHash (hs.newtype)

// A path in a repo is stored as a list of elements. This is so that the sort
// order of paths is the same as that of a tree traversal, so that deltas on
// manifests can be applied in a streaming way.
typedef binary MPathElement (hs.newtype)
typedef list<MPathElement> MPath (hs.newtype)
