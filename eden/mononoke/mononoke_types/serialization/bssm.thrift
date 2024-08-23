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

include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"

// Basename suffix manifest stores file trees in a way that allows fast filtering
// based on suffix of basenames as well as directory prefix of root.
// See docs/basename_suffix_skeleton_manifest.md for more documentation on this.
//
// BssmV3 is an optimized version of Bssm that differs from it in two ways:
//
// 1) It uses ShardedMapV2 instead of ShardedMap which avoids creating un-cachable blobs,
// instead dividing the manifest into closely sized blobs.
//
// 2) Stores the sharded map inlined without a layer of indirection, and relies only
// on the sharded map to decide which parts of the manifest should be inlined and
// which should be stored in a separate blob. This avoids the large number of tiny
// blobs that Bssm creates due to how unique basenames tend to be.
struct BssmV3File {} (rust.exhaustive)
struct BssmV3Directory {
  1: sharded_map.ShardedMapV2Node subentries;
} (rust.exhaustive)

union BssmV3Entry {
  1: BssmV3File file;
  2: BssmV3Directory directory;
} (rust.exhaustive)
