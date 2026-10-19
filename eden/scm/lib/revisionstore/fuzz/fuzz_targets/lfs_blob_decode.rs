/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Read a blob back out of an `LfsStore` whose chunk records have been
//! corrupted.
//!
//! Targets `LfsIndexedLogBlobsStore::get`: the mincode decode of
//! `LfsIndexedLogBlobsEntry` and the loop that stitches the decoded chunks back
//! into one blob. `range` and `data` are independent on-disk fields, and the
//! stitching indexes `data` with offsets derived from `range` - that pairing
//! has already been wrong once.
//!
//! As in `entry_decode`, records are patched before they are appended rather
//! than after: `indexedlog` checksums what it stores, so bytes patched on disk
//! are rejected below revisionstore and the decoder never runs.

#![no_main]

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;

use fuzzutil::corrupt::apply_patches;
use indexedlog::log::IndexOutput;
use libfuzzer_sys::fuzz_target;
use minibytes::Bytes;
use revisionstore::ContentHash;
use revisionstore::indexedlogutil::Store;
use revisionstore::indexedlogutil::StoreOpenOptions;
use revisionstore::scmstore::FileStoreBuilder;
use revisionstore::util::get_cache_path;
use revisionstore::util::get_lfs_blobs_path;
use tempfile::TempDir;
use types::Sha256;

/// Bytes per stored chunk, small enough that a modest blob is spread over
/// enough records for the stitching loop to have gaps and overlaps to handle.
const CHUNK_SIZE: usize = 16;

/// Length of the seed blob.
const BLOB_LEN: usize = 128;

/// Rotation limits `LfsIndexedLogBlobsStore` applies to its own log. The raw
/// store has to agree with them for the records it writes to be readable.
const MAX_LOG_COUNT: u8 = 4;
const MAX_BYTES_PER_LOG: u64 = 20_000_000_000 / 4;

static SEED: LazyLock<Seed> = LazyLock::new(build_seed);

/// A valid blob's chunk records, held in memory so an iteration can write a
/// corrupted copy without paying to rebuild the store.
struct Seed {
    hash: Sha256,
    /// Records with the leading sha256 stripped off. Patching only the tail
    /// keeps every record indexed under `hash`, so every input reaches the
    /// decoder rather than being dropped by the lookup.
    tails: Vec<Vec<u8>>,
}

/// Store config shared by the seed and the iterations.
///
/// `cleanup-old-logs-chance` is zeroed because `indexedlogutil` otherwise rolls
/// a random number on every rotated open and sometimes deletes logs, which
/// would make a crash depend on the RNG rather than on the input.
fn config(cache: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "scmstore.cleanup-old-logs-chance".to_string(),
            "0".to_string(),
        ),
        ("remotefilelog.reponame".to_string(), "fuzz".to_string()),
        (
            "remotefilelog.cachepath".to_string(),
            cache.to_string_lossy().into_owned(),
        ),
        ("lfs.blobschunksize".to_string(), CHUNK_SIZE.to_string()),
    ])
}

/// A `Store` carrying the same index definition and rotation limits
/// `LfsIndexedLogBlobsStore` uses, so records written through it are readable
/// by the real store.
fn raw_store(config: &BTreeMap<String, String>, path: &Path) -> Option<Store> {
    StoreOpenOptions::new(config)
        .create(true)
        .max_log_count(MAX_LOG_COUNT)
        .max_bytes_per_log(MAX_BYTES_PER_LOG)
        .index("sha256", |_| {
            vec![IndexOutput::Reference(0..Sha256::len() as u64)]
        })
        .rotated(path)
        .ok()
}

/// Resolve the blobs log inside the cache rooted at `cache`, creating the
/// directories `LfsStore` expects to find.
fn blobs_path(config: &BTreeMap<String, String>) -> Option<PathBuf> {
    let root = get_cache_path(config, &None::<PathBuf>).ok()??;
    get_lfs_blobs_path(root).ok()
}

fn build_seed() -> Seed {
    let dir = TempDir::new().expect("creating the seed dir should succeed");
    let config = config(dir.path());
    let path = blobs_path(&config).expect("resolving the seed blobs path should succeed");

    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let data: Vec<u8> = (0..BLOB_LEN)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let data = Bytes::from(data);
    let hash = ContentHash::sha256(&data).unwrap_sha256();

    let store = FileStoreBuilder::new(&config)
        .build_lfs_cache()
        .expect("creating the seed store should succeed")
        .expect("the seed config should configure a cache");
    store
        .add_blob(&hash, data.clone())
        .expect("adding the seed blob should succeed");
    store
        .flush()
        .expect("flushing the seed blob should succeed");
    drop(store);

    let raw = raw_store(&config, &path).expect("reopening the seed store should succeed");
    let guard = raw.read().expect("reading the seed store should succeed");
    let tails: Vec<Vec<u8>> = guard
        .iter()
        .map(|record| {
            record.expect("reading a seed record should succeed")[Sha256::len()..].to_vec()
        })
        .collect();
    assert_eq!(
        tails.len(),
        BLOB_LEN / CHUNK_SIZE,
        "the seed blob should be stored as one record per chunk"
    );

    // `raw_store` copies the index definition and rotation limits from
    // `LfsIndexedLogBlobsStore`. If those drift, every iteration would return
    // early without reaching the decoder, so check once that the unpatched
    // records, written the way an iteration writes them, read back as the blob.
    assert_eq!(
        read_back(&hash, &tails),
        Some(data),
        "the real store should read back the blob the raw store wrote"
    );

    Seed { hash, tails }
}

/// Append `hash` followed by each tail through a raw store in a fresh cache,
/// reopen the cache as an `LfsStore` and read the blob back. `None` covers
/// every failure: for corrupt records an error is the expected outcome.
fn read_back(hash: &Sha256, tails: &[Vec<u8>]) -> Option<Bytes> {
    let dir = TempDir::new().expect("creating a temp dir should succeed");
    let config = config(dir.path());
    let path = blobs_path(&config)?;
    let raw = raw_store(&config, &path)?;
    for tail in tails {
        let mut record = hash.as_ref().to_vec();
        record.extend_from_slice(tail);
        raw.append(record).ok()?;
    }
    raw.flush().ok()?;
    drop(raw);

    // `get_blob` deserializes every record indexed under the hash and stitches
    // them together.
    let store = FileStoreBuilder::new(&config).build_lfs_cache().ok()??;
    let blob = store.get_blob(hash, BLOB_LEN as u64).ok()??;
    Some(blob.into_bytes())
}

fuzz_target!(|input: &[u8]| {
    let mut tails = SEED.tails.clone();
    apply_patches(&mut tails, input, 0);
    std::hint::black_box(read_back(&SEED.hash, &tails));
});
