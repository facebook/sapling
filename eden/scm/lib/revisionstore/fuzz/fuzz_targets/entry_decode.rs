/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Decode a corrupt revisionstore entry out of an otherwise healthy store.
//!
//! Targets `Entry::from_bytes` (node, name length, body length and the body
//! slice), `InternalMetadata::read` (the metadata-list walk) and
//! `Entry::calculate_content` (lz4 decompression), reached through
//! `IndexedLogHgIdDataStore::get_raw_entry` and `ToKeys::to_keys`.
//!
//! The entry is patched before it is appended, not after, because
//! `indexedlog` checksums what it stores: corrupting bytes already on disk
//! fails the xxhash check in `Log::read_entry_from_buf` and the revisionstore
//! decoder never runs.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use revisionstore::HgIdDataStore;
use revisionstore::LocalStore;
use revisionstore::StoreKey;
use revisionstore::StoreType;
use revisionstore::ToKeys;
use types::HgId;
use types::Key;
use types::RepoPathBuf;

mod corrupt;

static SEED: LazyLock<Vec<Vec<u8>>> = LazyLock::new(corrupt::serialized_entries);

fuzz_target!(|input: &[u8]| {
    let shape = input.first().copied().unwrap_or(0) as usize % SEED.len();
    let mut payload = [SEED[shape].clone()];
    corrupt::apply_patches(&mut payload, input, HgId::len());
    let [payload] = payload;

    let dir = tempfile::TempDir::new().expect("creating a temp dir should succeed");
    if !corrupt::append_raw(dir.path(), &payload) {
        return;
    }

    let Some(store) = corrupt::open_store(dir.path(), StoreType::Permanent) else {
        return;
    };

    let node = HgId::from_slice(&payload[..HgId::len()]).expect("the node id survives patching");
    let key = StoreKey::hgid(Key::new(RepoPathBuf::new(), node));

    // `get` decodes the entry and decompresses its body; `to_keys` decodes
    // every entry in the log. Both discard errors, which are the expected
    // outcome for corrupt data.
    std::hint::black_box(store.get(key.clone()).is_ok());
    std::hint::black_box(store.get_missing(&[key]).is_ok());
    std::hint::black_box(store.to_keys().len());
});
