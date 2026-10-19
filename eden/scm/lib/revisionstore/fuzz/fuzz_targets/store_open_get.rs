/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Open a corrupted `IndexedLogHgIdDataStore` directory and read from it.
//!
//! This is the frame the EdenFS crash reports name: `Log::lookup` called from
//! `IndexedLogHgIdDataStore::get_raw_entry`. The seed is a rotated store built
//! the way the blob cache is, so a lookup walks the older logs that
//! `RotateLogLookupIter` opens lazily, and `open_with_repair` gets a chance to
//! rewrite whatever it decides is broken.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use revisionstore::HgIdDataStore;
use revisionstore::IndexedLogHgIdDataStore;
use revisionstore::LocalStore;
use revisionstore::StoreKey;
use revisionstore::StoreType;
use revisionstore::ToKeys;
use types::Key;
use types::RepoPathBuf;

mod corrupt;

static SEED: LazyLock<corrupt::Seed> = LazyLock::new(build_seed);

fn build_seed() -> corrupt::Seed {
    // Fixed epoch and no lag timestamp, so the snapshot is byte-identical
    // across processes and offset patches replay the same way.
    fuzzutil::configure_indexedlog_for_fuzzing();
    let dir = tempfile::TempDir::new().expect("creating the seed dir should succeed");
    let store = corrupt::open_store(dir.path(), StoreType::Rotated)
        .expect("creating the seed store should succeed");
    for i in 0..corrupt::SEED_ENTRY_COUNT {
        store
            .put_entry(corrupt::entry_for(i))
            .expect("appending to the seed store should succeed");
        if (i + 1) % corrupt::SEED_BATCH_SIZE == 0 {
            store
                .flush_log()
                .expect("flushing the seed store should succeed");
        }
    }
    store
        .flush_log()
        .expect("flushing the seed store should succeed");
    drop(store);
    corrupt::Seed::snapshot(dir.path())
}

fn probe(store: &IndexedLogHgIdDataStore, selector: usize) {
    for step in 0..corrupt::PROBE_KEYS {
        let node = corrupt::node_for(selector.wrapping_add(step * 97) % corrupt::SEED_ENTRY_COUNT);
        let key = StoreKey::hgid(Key::new(RepoPathBuf::new(), node));
        std::hint::black_box(store.get(key.clone()).is_ok());
        std::hint::black_box(store.get_missing(&[key]).is_ok());
    }
    std::hint::black_box(store.to_keys().len());
}

fuzz_target!(|input: &[u8]| {
    let dir = SEED.materialize(input);
    if let Some(store) = corrupt::open_store(dir.path(), StoreType::Rotated) {
        probe(&store, input.first().copied().unwrap_or(0) as usize);
    }
});
