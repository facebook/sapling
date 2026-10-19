/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Shared helpers for the `IndexedLogHgIdDataStore` read path fuzz targets.
//!
//! `indexedlog` verifies an xxhash over every entry before handing it back, so
//! patching a stored entry in place gets the read rejected a layer below
//! revisionstore and the decoder is never reached. The two targets therefore
//! corrupt at different levels:
//!
//! - `entry_decode` patches a valid serialized entry and appends the result
//!   through the store's own write path, which recomputes the checksum over
//!   the patched bytes. Every input reaches `Entry::from_bytes`.
//! - `store_open_get` patches the container files of a valid store instead,
//!   covering the `Log::lookup` frame the crash reports name.
//!
//! The patch-record decoder and the seed snapshot are `fuzzutil::corrupt`,
//! shared with the indexedlog and metalog targets; this module holds what is
//! specific to revisionstore's stores.

// Every target compiles its own copy of this module and uses a different part
// of it.
#![allow(dead_code, unused_imports)]

use std::collections::BTreeMap;
use std::path::Path;

pub use fuzzutil::corrupt::Seed;
pub use fuzzutil::corrupt::apply_patches;
use indexedlog::log::IndexOutput;
use minibytes::Bytes;
use revisionstore::IndexedLogHgIdDataStore;
use revisionstore::IndexedLogHgIdDataStoreConfig;
use revisionstore::Metadata;
use revisionstore::StoreType;
use revisionstore::indexedlogdatastore::Entry;
use revisionstore::indexedlogutil::Store;
use revisionstore::indexedlogutil::StoreOpenOptions;
use storemodel::SerializationFormat;
use tempfile::TempDir;
use types::HgId;

/// Entries written to a seed store.
pub const SEED_ENTRY_COUNT: usize = 256;

/// Entries appended between flushes. Each flush adds a link to the index
/// checksum chain that the next open has to walk.
pub const SEED_BATCH_SIZE: usize = 32;

/// Keys looked up per iteration.
pub const PROBE_KEYS: usize = 4;

/// Distinct entry shapes the `entry_decode` seed offers, covering compressed
/// and incompressible bodies, present and absent metadata fields, and ACL
/// children indices.
pub const ENTRY_SHAPES: usize = 16;

/// Rotation limits, small enough that a seed of a few hundred entries fills
/// every log slot. `IndexedLogHgIdDataStore` reads these from the config when
/// `IndexedLogHgIdDataStoreConfig` leaves them unset.
const MAX_BYTES_PER_LOG: &str = "2048";
const MAX_LOG_COUNT: &str = "4";

/// Store config shared by the targets.
///
/// `cleanup-old-logs-chance` is zeroed because `indexedlogutil` otherwise rolls
/// a random number on every rotated open and sometimes deletes logs, which
/// would make a crash depend on the RNG rather than on the input.
pub fn config() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("scmstore.cleanup-old-logs-chance", "0"),
        ("indexedlog.hgdata.max-bytes-per-log", MAX_BYTES_PER_LOG),
        ("indexedlog.hgdata.max-log-count", MAX_LOG_COUNT),
    ])
}

fn log_config() -> IndexedLogHgIdDataStoreConfig {
    IndexedLogHgIdDataStoreConfig {
        max_log_count: None,
        max_bytes_per_log: None,
        max_bytes: None,
        btrfs_compression: false,
    }
}

/// Open the store under test. `None` covers the errors a corrupt directory is
/// expected to produce.
pub fn open_store(path: &Path, store_type: StoreType) -> Option<IndexedLogHgIdDataStore> {
    IndexedLogHgIdDataStore::new(
        &config(),
        path,
        &log_config(),
        store_type,
        SerializationFormat::Hg,
    )
    .ok()
}

/// A `Store` carrying the same index definition `IndexedLogHgIdDataStore` uses,
/// so bytes written through it are readable by the real store.
fn raw_store(path: &Path) -> Option<Store> {
    StoreOpenOptions::new(&config())
        .create(true)
        .index("node", |_| {
            vec![IndexOutput::Reference(0..HgId::len() as u64)]
        })
        .permanent(path)
        .ok()
}

/// Deterministic node id.
///
/// The leading bytes advance at different rates so nodes share prefixes of
/// varying length, giving the radix index interior nodes rather than a flat
/// set of leaves.
pub fn node_for(i: usize) -> HgId {
    let mut bytes = [0u8; HgId::len()];
    bytes[0] = (i / 97) as u8;
    bytes[1] = (i / 13) as u8;
    let mut state = seed_state(i);
    for byte in bytes[2..].iter_mut() {
        *byte = next_byte(&mut state);
    }
    HgId::from_byte_array(bytes)
}

/// Deterministic entry, varying every field the decoder parses.
pub fn entry_for(i: usize) -> Entry {
    let len = 64 + i % 192;
    let content: Vec<u8> = if i.is_multiple_of(2) {
        // Compresses to a short lz4 frame.
        vec![b'a'; len]
    } else {
        // Incompressible, so the stored body stays roughly this long.
        let mut state = seed_state(i);
        (0..len).map(|_| next_byte(&mut state)).collect()
    };

    let metadata = Metadata {
        size: (!i.is_multiple_of(3)).then_some(content.len() as u64),
        flags: i.is_multiple_of(16).then_some(Metadata::LFS_FLAG),
    };

    let mut entry = Entry::new(node_for(i), Bytes::from(content), metadata);
    if i.is_multiple_of(8) {
        entry.set_acl_children_indices((0..(i % 5) as u32).collect());
    }
    entry
}

fn seed_state(i: usize) -> u64 {
    (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1
}

fn next_byte(state: &mut u64) -> u8 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state as u8
}

/// Round-trip valid entries through a store and read back the exact bytes the
/// writer produced.
///
/// `Entry::serialize` is private to revisionstore, so the store's own write
/// path is the only way to obtain a serialized entry to patch.
pub fn serialized_entries() -> Vec<Vec<u8>> {
    let dir = TempDir::new().expect("creating the seed dir should succeed");
    let store = raw_store(dir.path()).expect("creating the seed store should succeed");
    for i in 0..ENTRY_SHAPES {
        entry_for(i)
            .write_to_log(&store)
            .expect("appending to the seed store should succeed");
    }
    store
        .flush()
        .expect("flushing the seed store should succeed");

    let guard = store.read().expect("reading the seed store should succeed");
    let entries: Vec<Vec<u8>> = guard
        .iter()
        .map(|entry| entry.expect("reading a seed entry should succeed").to_vec())
        .collect();
    assert_eq!(
        entries.len(),
        ENTRY_SHAPES,
        "every seed entry should read back"
    );
    entries
}

/// Append `payload` verbatim through the store's write path, so `indexedlog`
/// computes its checksum over the patched bytes rather than rejecting them.
///
/// Returns whether the payload made it to disk; a store that refused it is not
/// a finding.
pub fn append_raw(path: &Path, payload: &[u8]) -> bool {
    let Some(store) = raw_store(path) else {
        return false;
    };
    store.append(payload.to_vec()).is_ok() && store.flush().is_ok()
}
