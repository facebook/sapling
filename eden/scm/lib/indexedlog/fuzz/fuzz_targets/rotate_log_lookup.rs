/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Open a corrupted `RotateLog` directory and read from it.
//!
//! The seed mirrors how `IndexedLogHgIdDataStore` configures its store: four
//! rotated logs, `create` enabled, and a 20-byte node index. Draining the
//! lookup iterator walks the older logs through
//! `RotateLogLookupIter::load_next_log`, which opens each one lazily - the
//! frame that dominates the EdenFS crash reports for this crate.

#![no_main]

use std::sync::LazyLock;

use indexedlog::log::IndexOutput;
use indexedlog::rotate::OpenOptions;
use indexedlog::rotate::RotateLog;
use libfuzzer_sys::fuzz_target;

mod corrupt;

/// Small enough that a batch of appends rotates the writable log, so the seed
/// ends up with every log slot populated.
const MAX_BYTES_PER_LOG: u64 = 2048;

const MAX_LOG_COUNT: u8 = 4;

static SEED: LazyLock<corrupt::Seed> = LazyLock::new(build_seed);

fn open_options() -> OpenOptions {
    OpenOptions::new()
        .max_log_count(MAX_LOG_COUNT)
        .max_bytes_per_log(MAX_BYTES_PER_LOG)
        .create(true)
        .index("node", |_| {
            vec![IndexOutput::Reference(0..corrupt::KEY_LEN as u64)]
        })
}

fn build_seed() -> corrupt::Seed {
    // Fixed epoch and no lag timestamp, so the snapshot is byte-identical
    // across processes and offset patches replay the same way.
    fuzzutil::configure_indexedlog_for_fuzzing();
    let dir = tempfile::TempDir::new().expect("creating the seed dir should succeed");
    let mut rotate_log = open_options()
        .open(dir.path())
        .expect("creating the seed rotate log should succeed");
    for i in 0..corrupt::SEED_ENTRY_COUNT {
        rotate_log
            .append(corrupt::entry_for(i))
            .expect("appending to the seed rotate log should succeed");
        if (i + 1) % corrupt::SEED_BATCH_SIZE == 0 {
            rotate_log
                .sync()
                .expect("syncing the seed rotate log should succeed");
        }
    }
    rotate_log
        .sync()
        .expect("syncing the seed rotate log should succeed");
    corrupt::Seed::snapshot(dir.path())
}

fn probe(rotate_log: &RotateLog, selector: usize) {
    for step in 0..corrupt::PROBE_KEYS {
        let key = corrupt::key_for(selector.wrapping_add(step * 97) % corrupt::SEED_ENTRY_COUNT);
        if let Ok(iter) = rotate_log.lookup(0, key.to_vec()) {
            corrupt::drain(iter);
        }
    }
    corrupt::drain(rotate_log.iter());
}

fuzz_target!(|input: &[u8]| {
    let dir = SEED.materialize(input);
    if let Ok(rotate_log) = open_options().open(dir.path()) {
        probe(&rotate_log, input.first().copied().unwrap_or(0) as usize);
    }
});
