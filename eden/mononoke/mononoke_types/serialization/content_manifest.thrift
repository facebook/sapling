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
include "eden/mononoke/mononoke_types/serialization/bonsai.thrift"
include "eden/mononoke/mononoke_types/serialization/sharded_map.thrift"

struct ContentManifestFile {
  1: id.ContentId content_id;
  2: bonsai.FileType file_type;
  3: i64 size;
} (rust.exhaustive)

struct ContentManifestDirectory {
  1: id.ContentManifestId id;
} (rust.exhaustive)

union ContentManifestEntry {
  1: ContentManifestFile file;
  2: ContentManifestDirectory directory;
}

// Content-addressed manifest.
//
// Content manifests form a manifest tree where the type and contents of files
// and directories (but not their history) forms their identity.  Content
// manifest identities change when any file content is changed.
struct ContentManifest {
  // Map of MPathElement -> ContentManifestEntry
  1: sharded_map.ShardedMapV2Node subentries;
} (rust.exhaustive)
