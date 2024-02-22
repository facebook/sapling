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
include "eden/mononoke/mononoke_types/serialization/data.thrift"

struct ContentChunkPointer {
  1: id.ContentChunkId chunk_id;
  2: i64 size;
} (rust.exhaustive)

// When a file is chunked, we represent it as a list of its chunks, as well as
// its ContentId.
struct ChunkedFileContents {
  // The ContentId is here to ensure we can reproduce the ContentId from the
  // FileContents representation in Mononoke, which would normally require
  // hashing the contents (but we obviously can't do that here, since we don't
  // have the contents).
  1: id.ContentId content_id;
  2: list<ContentChunkPointer> chunks;
} (rust.exhaustive)

union FileContents {
  // Plain uncompressed bytes - WYSIWYG.
  1: data.LargeBinary Bytes;
  // References to Chunks (stored as FileContents, too).
  2: ChunkedFileContents Chunked;
}

union ContentChunk {
  1: data.LargeBinary Bytes;
}

// Payload of object which is an alias
union ContentAlias {
  1: id.ContentId ContentId; // File content alias
}

// Metadata and properties associated with a file.
// NOTE: Fields 1 through 10 will always be written by Mononoke, and Mononoke
// will expect them to be present when reading ContentMetadataV2 structs back
// from its Filestore. They're marked optional so we can report errors if
// they're absent at runtime (as opposed to letting Thrift give us a default
// values).
struct ContentMetadataV2 {
  // ContentId we're providing metadata for
  1: optional id.ContentId content_id;
  // total_size is needed to make GitSha1 meaningful, but generally useful
  2: optional i64 total_size;
  // SHA1 hash of the content
  3: optional id.Sha1 sha1;
  // SHA256 hash of the content
  4: optional id.Sha256 sha256;
  // Git SHA1 hash of the content
  5: optional id.GitSha1 git_sha1;
  // Is the file binary?
  // NOTE: A file is defined as binary if it contains a null byte
  // as part of its content
  6: optional bool is_binary;
  // Does the file contain only ASCII characters?
  7: optional bool is_ascii;
  // Is the file content UTF-8 encoded?
  8: optional bool is_utf8;
  // Does the file end in a newline?
  9: optional bool ends_in_newline;
  // How many newlines does the file have?
  10: optional i64 newline_count;
  // The first UTF-8 encoded line of the file content OR
  // UTF-8 string equivalent of the first 64 bytes,
  // whichever is the shortest. If is_utf8 is false, the
  // first_line is None
  11: optional string first_line;
  // Is the file auto-generated? i.e. does it have the '@'+'generated' tag
  12: optional bool is_generated;
  // Is the file partially-generated? i.e. does it have the '@'+'partially-generated' tag
  13: optional bool is_partially_generated;
  // Blake3 hash of the file seeded with the global thrift constant in fbcode/blake3.thrift
  14: optional id.Blake3 seeded_blake3;
} (rust.exhaustive)
