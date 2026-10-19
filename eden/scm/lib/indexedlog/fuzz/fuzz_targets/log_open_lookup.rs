/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Open a corrupted `Log` directory and read from it.
//!
//! Targets `Log::open` (primary header, `meta` parsing, `Index` open and the
//! `MemChecksum` chain walk), `Log::lookup` and `Log::lookup_prefix` (radix
//! tree walk plus per-read checksum verification), and `Log::iter` (entry
//! header and xxhash parsing).

#![no_main]

use std::sync::LazyLock;

use indexedlog::log::IndexOutput;
use indexedlog::log::Log;
use indexedlog::log::OpenOptions;
use libfuzzer_sys::fuzz_target;

mod corrupt;

static SEED: LazyLock<corrupt::Seed> = LazyLock::new(build_seed);

fn open_options() -> OpenOptions {
    OpenOptions::new().index("node", |_| {
        vec![IndexOutput::Reference(0..corrupt::KEY_LEN as u64)]
    })
}

fn build_seed() -> corrupt::Seed {
    // Fixed epoch and no lag timestamp, so the snapshot is byte-identical
    // across processes and offset patches replay the same way.
    fuzzutil::configure_indexedlog_for_fuzzing();
    let dir = tempfile::TempDir::new().expect("creating the seed dir should succeed");
    let mut log = open_options()
        .create(true)
        .open(dir.path())
        .expect("creating the seed log should succeed");
    for i in 0..corrupt::SEED_ENTRY_COUNT {
        log.append(corrupt::entry_for(i))
            .expect("appending to the seed log should succeed");
        if (i + 1) % corrupt::SEED_BATCH_SIZE == 0 {
            log.sync().expect("syncing the seed log should succeed");
        }
    }
    log.sync().expect("syncing the seed log should succeed");
    corrupt::Seed::snapshot(dir.path())
}

fn probe(log: &Log, selector: usize) {
    for step in 0..corrupt::PROBE_KEYS {
        let key = corrupt::key_for(selector.wrapping_add(step * 97) % corrupt::SEED_ENTRY_COUNT);
        if let Ok(iter) = log.lookup(0, key) {
            corrupt::drain(iter);
        }
        if let Ok(iter) = log.lookup_prefix(0, &key[..2]) {
            for matched in iter {
                let Ok((_key, values)) = matched else {
                    break;
                };
                corrupt::drain(values);
            }
        }
    }
    corrupt::drain(log.iter());
}

fuzz_target!(|input: &[u8]| {
    let dir = SEED.materialize(input);
    if let Ok(log) = open_options().open(dir.path()) {
        probe(&log, input.first().copied().unwrap_or(0) as usize);
    }
});
