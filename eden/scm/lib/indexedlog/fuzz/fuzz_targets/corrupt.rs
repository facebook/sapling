/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Seed data for the indexedlog on-disk format fuzz targets.
//!
//! Both targets have the same shape: build one pristine on-disk structure per
//! process, then per iteration materialize a fuzzer-corrupted copy of it in a
//! fresh temporary directory and re-open it. The corruption itself is
//! `fuzzutil::corrupt`, shared with the metalog and revisionstore targets.
//!
//! Starting from a valid structure is what gets depth here. Pure random bytes
//! are rejected by the `indexedlog0\0` primary header check, or by the `meta`
//! header, and never reach the index radix walk or the `MemChecksum`
//! verification that the crash reports point at.

pub use fuzzutil::corrupt::Seed;
use indexedlog::Result;

/// Index key length, matching the 20-byte node index `IndexedLogHgIdDataStore`
/// puts on its `RotateLog`.
pub const KEY_LEN: usize = 20;

/// Entries appended to a seed structure.
pub const SEED_ENTRY_COUNT: usize = 512;

/// Entries appended between syncs. Every sync adds a link to the index
/// checksum chain, which is what `MemChecksum::new` walks on the next open.
pub const SEED_BATCH_SIZE: usize = 64;

/// Keys looked up per iteration.
pub const PROBE_KEYS: usize = 4;

/// Entries drained from a single iterator. A corrupt index can yield a very
/// long value chain; the cap keeps throughput up without suppressing crashes,
/// which happen while producing an entry rather than after many of them.
const MAX_DRAIN: usize = 1 << 16;

/// Deterministic index key.
///
/// The first two bytes are derived from `i` at different rates so keys share
/// prefixes of varying length, giving the radix index interior nodes rather
/// than a flat set of leaves. Every eighth key repeats its predecessor, so
/// some keys resolve to a chain of values instead of a single one.
pub fn key_for(i: usize) -> [u8; KEY_LEN] {
    let i = if i.is_multiple_of(8) {
        i.saturating_sub(1)
    } else {
        i
    };
    let mut key = [0u8; KEY_LEN];
    key[0] = (i / 97) as u8;
    key[1] = (i / 13) as u8;
    let mut state = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for byte in key[2..].iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }
    key
}

/// Deterministic entry: the index key followed by a body, mirroring how
/// `IndexedLogHgIdDataStore` stores a node followed by its content.
pub fn entry_for(i: usize) -> Vec<u8> {
    let mut entry = key_for(i).to_vec();
    entry.extend((0..16u8).map(|j| (i as u8).wrapping_mul(31).wrapping_add(j)));
    entry
}

/// Read every entry an iterator yields, stopping at the first error.
///
/// Errors are the expected outcome for corrupt data and are not findings, so
/// they are discarded. Entry bytes are summed rather than dropped so that
/// reads through the memory map actually happen and out-of-bounds accesses are
/// visible to the sanitizers.
pub fn drain<'a>(iter: impl Iterator<Item = Result<&'a [u8]>>) {
    let mut sum = 0u8;
    for entry in iter.take(MAX_DRAIN) {
        let Ok(entry) = entry else {
            break;
        };
        sum = entry.iter().fold(sum, |acc, byte| acc.wrapping_add(*byte));
    }
    std::hint::black_box(sum);
}
