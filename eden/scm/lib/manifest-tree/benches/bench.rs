/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use quickcheck::{Arbitrary, StdGen};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;

use minibench::{bench, elapsed};
use pathmatcher::AlwaysMatcher;
use types::{testutil::generate_repo_paths, HgId, RepoPathBuf};

use manifest::{FileMetadata, Manifest};
use manifest_tree::{testutil::*, TreeManifest, TreeStore};

const INIT_SET_COUNT: usize = 4_000_000;
const OP_COUNT: usize = 1_000_000;

// See https://github.com/rust-lang/rust/issues/64102
pub fn black_box<T>(dummy: T) -> T {
    unsafe {
        let ret = std::ptr::read_volatile(&dummy);
        std::mem::forget(dummy);
        ret
    }
}

pub fn generate_entries<G: quickcheck::Gen>(
    entry_count: usize,
    qc_gen: &mut G,
) -> Vec<(RepoPathBuf, FileMetadata)> {
    let repo_paths = generate_repo_paths(entry_count, qc_gen);
    let mut result = Vec::with_capacity(entry_count);
    for path in repo_paths.into_iter() {
        let hgid = HgId::arbitrary(qc_gen);
        result.push((path, FileMetadata::regular(hgid)));
    }
    result
}

pub fn finalize(
    store: &TestStore,
    manifest: &mut TreeManifest,
    parent_manifests: Vec<&TreeManifest>,
) -> HgId {
    let mut manifest_id = Default::default();
    for (path, hgid, raw, _, _) in manifest.finalize(parent_manifests).unwrap() {
        store.insert(&path, hgid, raw).unwrap();
        if path.is_empty() {
            manifest_id = hgid;
        }
    }
    manifest_id
}

// Run with: cargo bench --features for-tests
// In this benchmark we focus on the `finalize` model of writing to storage.
// This model required the caller to write the data to a store that they have.
fn main() {
    let rng = ChaChaRng::from_seed([0u8; 32]);
    let mut qc_gen = StdGen::new(rng, 10);
    let store = Arc::new(TestStore::new());
    let entries = generate_entries(INIT_SET_COUNT + OP_COUNT, &mut qc_gen);
    let initial_entries = &entries[..INIT_SET_COUNT];
    let op_entries = &entries[INIT_SET_COUNT..];
    let mut initial_manifest = TreeManifest::ephemeral(store.clone());
    for (path, file_metadata) in initial_entries.iter() {
        initial_manifest
            .insert(path.to_owned(), *file_metadata)
            .unwrap();
    }
    println!("initial_entry_cnt = {}", initial_entries.len());
    // Iterate through the tree before it gets committed to storage
    bench("iterate_files_ephemeral", || {
        // In this block we rely on the fact that we have access to the initial manifest before
        // it is finalized. This is an optimization to redo file insertion.
        elapsed(|| {
            for file in initial_manifest.files(&AlwaysMatcher::new()) {
                black_box(file).unwrap();
            }
        })
    });
    let initial_manifest_id = finalize(&store, &mut initial_manifest, vec![]);

    // Iterate through the durable entries that are all loaded in memory
    bench("iterate_files_durable_in_memory", || {
        // In this block we rely on the fact that we did not take the initial manifest out of
        // memory. This is just an optimization to avoid reloading a manifest.
        let manifest = initial_manifest.clone();
        elapsed(|| {
            for file in manifest.files(&AlwaysMatcher::new()) {
                black_box(file).unwrap();
            }
        })
    });

    // Iterate through the durable entries while loading them from storage
    bench("iterate_files_durable_load", || {
        let manifest = TreeManifest::durable(store.clone(), initial_manifest_id);
        elapsed(|| {
            for file in manifest.files(&AlwaysMatcher::new()) {
                black_box(file).unwrap();
            }
        })
    });

    // Execute OP_COUNT insertions.
    bench("insert", || {
        let mut manifest = initial_manifest.clone();
        elapsed(|| {
            for (path, file_metadata) in op_entries.iter() {
                manifest.insert(path.to_owned(), *file_metadata).unwrap();
            }
        })
    });

    // Finalize a tree with OP_COUNT new leaves.
    bench("finalize", || {
        let mut manifest = initial_manifest.clone();
        for (path, file_metadata) in op_entries.iter() {
            manifest.insert(path.to_owned(), *file_metadata).unwrap();
        }
        elapsed(|| {
            for x in manifest.finalize(vec![&initial_manifest]).unwrap() {
                black_box(x);
            }
        })
    });

    // Remove the previously added files.
    bench("remove", || {
        let mut manifest = initial_manifest.clone();
        elapsed(|| {
            for (path, _) in initial_entries.iter().take(OP_COUNT) {
                manifest.remove(path).unwrap();
            }
        })
    });
}
