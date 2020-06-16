/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Independent single data value.
union SingleValue {
    1: binary Raw,
    2: binary Zstd,
}

// Represents dictionary encoded Zstandard blob. dict_key must point to a
// SingleValue::Zstd encoded blob otherwise there will be an Error on get.
//
// When packing, dict_key will usually point to an earlier version of the same
// key, resulting in zstd being the delta-like Zstd encoded differences between
// the versions. Alternatively the packer might chose to point to shared
// dictionary, e.g. use a dictionary per repo+file extension).
//
// For the initial packer the dict_key must precede this item in the
// list<PackedEntry>, otherwise there will be an Error on get(). Later this may
// be relaxed to so as to give the option of shared dictionaries between packs,
// in which case the blob for a dict_key that is not preceding will be resolved
// via blobstore get().
//
struct ZstdFromDictValue {
    1: string dict_key,
    2: binary zstd,
}

// Packed values might not take any advantage of delta compression, but its
// there if the packer decides its most efficient for the blob
union PackedValue {
    1: SingleValue Single,
    2: ZstdFromDictValue ZstdFromDict,
}

// One packed entry,  the key being the blobstore key and the data being
// the packed value.
struct PackedEntry {
    1: string key,
    2: PackedValue data,
}

struct PackedFormat {
    // The key the PackedFormat is stored under in underlying storage
    // Used for caching.
    // We store it in the blob as underlying store may not reveal link
    // contents. (i.e. store's links are hardlink-like rather than symlink-like)
    // It is a string rather than binary as that is what blobstore api and
    // memcache apis expect
    1: string key,
    // Possibly multiple entries,  should contain at least the key the blob was
    // loaded via.  This is a list rather than a map as shouldn't need to index
    // all entries on get(), only those up to the one that is being requested.
    2: list<PackedEntry> entries,
}

// Discriminated union with the variant forms, for now we handle single
// independent values or a list of packed entries.
// The blobstore would theoretically still work (super slowly/with OOMs)
// if all blobs were stored in one list<PackedEntry>
union StorageFormat {
    1: SingleValue Single,
    2: PackedFormat Packed,
}

// At-rest form for mononoke blobs, top level struct for persistance.
// Recommended that top level type is struct even though logically
// the union would work.
struct StorageEnvelope {
    1: StorageFormat storage
}
