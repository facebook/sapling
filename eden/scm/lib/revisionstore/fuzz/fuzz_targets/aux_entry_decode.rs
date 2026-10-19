/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Decode a corrupt aux entry out of an otherwise healthy `AuxStore`.
//!
//! Targets `indexedlogauxstore::deserialize`: the version byte that selects
//! between three on-disk layouts, the VLQ `total_size`, the fixed-width hash
//! reads and - the part that has already been wrong once - the optional file
//! header metadata, whose VLQ size is added to the cursor position and used to
//! slice `Bytes`.
//!
//! As in `entry_decode`, the entry is patched before it is appended rather than
//! after: `indexedlog` checksums what it stores, so bytes patched on disk are
//! rejected below revisionstore and `deserialize` never runs.

#![no_main]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use indexedlog::log::IndexOutput;
use libfuzzer_sys::fuzz_target;
use minibytes::Bytes;
use revisionstore::AuxStore;
use revisionstore::StoreType;
use revisionstore::indexedlogutil::Store;
use revisionstore::indexedlogutil::StoreOpenOptions;
use revisionstore::scmstore::FileAuxData;
use tempfile::TempDir;
use types::HgId;

mod corrupt;

/// Distinct entry shapes the seed offers, covering `total_size` VLQ widths of
/// two to ten bytes and absent, empty and populated file header metadata.
const ENTRY_SHAPES: usize = 12;

static SEED: LazyLock<Vec<Vec<u8>>> = LazyLock::new(serialized_entries);

/// `cleanup-old-logs-chance` is zeroed so no open rolls a random number and
/// deletes a log, which would make a crash depend on the RNG rather than on
/// the input.
fn config() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([("scmstore.cleanup-old-logs-chance", "0")])
}

/// A `Store` carrying the same index definition `AuxStore` uses, so bytes
/// written through it are readable by the real store.
fn raw_store(path: &Path) -> Option<Store> {
    StoreOpenOptions::new(&config())
        .create(true)
        .index("node", |_| {
            vec![IndexOutput::Reference(0..HgId::len() as u64)]
        })
        .permanent(path)
        .ok()
}

/// Deterministic aux data, varying every field the decoder parses.
fn aux_for(i: usize) -> FileAuxData {
    let file_header_metadata = match i % 3 {
        0 => None,
        1 => Some(Bytes::new()),
        _ => Some(Bytes::from(vec![b'm'; i * 7])),
    };

    FileAuxData {
        // Encodes as a ten-byte VLQ down to a two-byte one; never one byte.
        total_size: u64::MAX >> (i % 9 * 7),
        file_header_metadata,
        ..Default::default()
    }
}

/// Round-trip valid entries through an `AuxStore` and read back the exact bytes
/// the writer produced.
///
/// `indexedlogauxstore::serialize` is private to revisionstore, so the store's
/// own write path is the only way to obtain a serialized entry to patch.
fn serialized_entries() -> Vec<Vec<u8>> {
    let dir = TempDir::new().expect("creating the seed dir should succeed");
    let store = AuxStore::new(dir.path(), &config(), StoreType::Permanent)
        .expect("creating the seed store should succeed");
    for i in 0..ENTRY_SHAPES {
        store
            .put(corrupt::node_for(i), &aux_for(i))
            .expect("appending to the seed store should succeed");
    }
    store
        .flush()
        .expect("flushing the seed store should succeed");
    drop(store);

    let raw = raw_store(dir.path()).expect("reopening the seed store should succeed");
    let guard = raw.read().expect("reading the seed store should succeed");
    let entries: Vec<Vec<u8>> = guard
        .iter()
        .map(|entry| entry.expect("reading a seed entry should succeed").to_vec())
        .collect();
    assert_eq!(
        entries.len(),
        ENTRY_SHAPES,
        "every seed entry should read back"
    );

    // `raw_store` copies the index definition from `AuxStore`. If the two
    // drift, every iteration would return early without reaching the decoder,
    // so check once that each unpatched entry, written the way an iteration
    // writes it, decodes back to the aux data it came from.
    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(
            read_back(entry),
            Some(aux_for(i)),
            "the real store should read back seed entry {i} the raw store wrote"
        );
    }
    entries
}

/// Append `payload` through a raw store in a fresh dir, reopen the dir as an
/// `AuxStore` and look up the node in its first 20 bytes. `None` covers every
/// failure: for a corrupt entry an error is the expected outcome.
fn read_back(payload: &[u8]) -> Option<FileAuxData> {
    let dir = TempDir::new().expect("creating a temp dir should succeed");
    let raw = raw_store(dir.path())?;
    raw.append(payload).ok()?;
    raw.flush().ok()?;
    drop(raw);

    let store = AuxStore::new(dir.path(), &config(), StoreType::Permanent).ok()?;

    // The lookup follows the patch: the index references the first 20 bytes,
    // which `apply_patches` may rewrite but never removes.
    let node = HgId::from_slice(&payload[..HgId::len()]).expect("the node id survives patching");

    // `get` deserializes the entry; `contains` only walks the index.
    let aux = store.get(&node).ok().flatten();
    std::hint::black_box(store.contains(node).is_ok());
    aux
}

fuzz_target!(|input: &[u8]| {
    let shape = input.first().copied().unwrap_or(0) as usize % SEED.len();
    let mut payload = [SEED[shape].clone()];
    corrupt::apply_patches(&mut payload, input, HgId::len());
    let [payload] = payload;
    std::hint::black_box(read_back(&payload));
});
