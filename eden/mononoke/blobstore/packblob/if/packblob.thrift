/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

typedef binary (rust.type = "bytes::Bytes") bytes

// Independent single data value.
union SingleValue {
  1: bytes Raw;
  2: bytes Zstd;
}

// Represents dictionary encoded Zstandard blob.
//
// dict_key is the key of a blob that was used to create a Zstandard
// dictionary, and whose contents must be available when the blob is
// decoded. If dict_key is in the same pack as this value, then it
// must be before this value in the pack.
//
// Further, it must be possible to decompress dict_key without
// decompressing this key - if that restriction is not met, then
// this key cannot be decompressed.
//
// The current implementation cannot fetch further packs to find
// dict_key; this limitation may be lifted later.
struct ZstdFromDictValue {
  1: string dict_key;
  2: bytes zstd;
} (rust.exhaustive)

// Packed values might not take any advantage of delta compression, but its
// there if the packer decides its most efficient for the blob
union PackedValue {
  1: SingleValue Single;
  2: ZstdFromDictValue ZstdFromDict;
}

// One packed entry,  the key being the blobstore key and the data being
// the packed value.
struct PackedEntry {
  1: string key;
  2: PackedValue data;
} (rust.exhaustive)

struct PackedFormat {
  // The key the PackedFormat is stored under in underlying storage
  // Used for caching, and may not exist in the underlying storage
  // but is the content-addressable value the pack would live at if
  // it had its own storage
  // We store it in the blob as underlying store may not reveal link
  // contents. (i.e. store's links are hardlink-like rather than symlink-like)
  // It is a string rather than binary as that is what blobstore api and
  // memcache apis expect
  1: string key;
  // Possibly multiple entries,  should contain at least the key the blob was
  // loaded via.  This is a list rather than a map as shouldn't need to index
  // all entries on get(), only those up to the one that is being requested.
  // All but the first entry should be ZstdFromDict, to maximize compression.
  // We do not expect significant gains from fewer blobs in the underlying store.
  2: list<PackedEntry> entries;
} (rust.exhaustive)

// Discriminated union with the variant forms, for now we handle single
// independent values or a list of packed entries.
// The blobstore would theoretically still work (super slowly/with OOMs)
// if all blobs were stored in one list<PackedEntry>
union StorageFormat {
  1: SingleValue Single;
  2: PackedFormat Packed;
}

// At-rest form for mononoke blobs, top level struct for persistance.
// Recommended that top level type is struct even though logically
// the union would work.
struct StorageEnvelope {
  1: StorageFormat storage;
} (rust.exhaustive)
