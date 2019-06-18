// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::utils::run_future;
use async_unit;
use failure_ext::Result;
use futures::future::Future;
use rand::{distributions::Normal, SeedableRng};
use rand_xorshift::XorShiftRng;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

use benchmark_lib::{new_benchmark_repo, DelaySettings, GenManifest};
use blobrepo::internal::{IncompleteFilenodes, MemoryManifestEntry, MemoryRootManifest};
use blobrepo::HgBlobEntry;
use context::CoreContext;
use fixtures::many_files_dirs;
use mercurial_types::{
    Entry, FileType, HgFileNodeId, HgManifestId, HgNodeHash, MPath, MPathElement, Type,
};
use mercurial_types_mocks::nodehash;
use mononoke_types::RepoPath;

fn insert_entry(tree: &MemoryManifestEntry, path: MPathElement, entry: MemoryManifestEntry) {
    match tree {
        MemoryManifestEntry::MemTree { changes, .. } => {
            let mut changes = changes.lock().expect("lock poisoned");
            changes.insert(path, Some(entry));
        }
        _ => panic!("Inserting into a non-Tree"),
    }
}

#[test]
fn empty_manifest() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let repo = many_files_dirs::getrepo(None);

        // Create an empty memory manifest
        let memory_manifest =
            MemoryRootManifest::new(ctx, repo, IncompleteFilenodes::new(), None, None)
                .wait()
                .expect("Could not create empty manifest");

        if let MemoryManifestEntry::MemTree {
            base_manifest_id,
            p1,
            p2,
            changes,
        } = memory_manifest.unittest_root()
        {
            let changes = changes.lock().expect("lock poisoned");
            assert!(base_manifest_id.is_none(), "Empty manifest had a baseline");
            assert!(p1.is_none(), "Empty manifest had p1");
            assert!(p2.is_none(), "Empty manifest had p2");
            assert!(changes.is_empty(), "Empty manifest had new entries changed");
        } else {
            panic!("Empty manifest is not a MemTree");
        }
    })
}

#[test]
fn load_manifest() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let repo = many_files_dirs::getrepo(None);

        let manifest_id = HgNodeHash::from_static_str("907f5b20e06dfb91057861d984423e84b64b5b7b")
            .expect("Could not get nodehash");

        // Load a memory manifest
        let memory_manifest = MemoryRootManifest::new(
            ctx.clone(),
            repo,
            IncompleteFilenodes::new(),
            Some(manifest_id),
            None,
        )
        .wait()
        .expect("Could not load manifest");

        if let MemoryManifestEntry::MemTree {
            base_manifest_id,
            p1,
            p2,
            changes,
        } = memory_manifest.unittest_root()
        {
            let changes = changes.lock().expect("lock poisoned");
            assert_eq!(
                *base_manifest_id,
                Some(manifest_id),
                "Loaded manifest had wrong base {:?}",
                base_manifest_id
            );
            assert_eq!(
                *p1,
                Some(manifest_id),
                "Loaded manifest had wrong p1 {:?}",
                p1
            );
            assert!(p2.is_none(), "Loaded manifest had p2");
            assert!(
                changes.is_empty(),
                "Loaded (unaltered) manifest has had entries changed"
            );
        } else {
            panic!("Loaded manifest is not a MemTree");
        }
    })
}

#[test]
fn save_manifest() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let repo = many_files_dirs::getrepo(None);

        // Create an empty memory manifest
        let memory_manifest = MemoryRootManifest::new(
            ctx.clone(),
            repo.clone(),
            IncompleteFilenodes::new(),
            None,
            None,
        )
        .wait()
        .expect("Could not create empty manifest");

        // Add an unmodified entry
        let dir_nodehash = HgNodeHash::from_static_str("907f5b20e06dfb91057861d984423e84b64b5b7b")
            .expect("Could not get nodehash");
        let dir = MemoryManifestEntry::MemTree {
            base_manifest_id: Some(dir_nodehash),
            p1: Some(dir_nodehash),
            p2: None,
            changes: Arc::new(Mutex::new(BTreeMap::new())),
        };
        let path =
            MPathElement::new(b"dir".to_vec()).expect("dir is no longer a valid MPathElement");
        insert_entry(&memory_manifest.unittest_root(), path.clone(), dir);

        let manifest_entry = memory_manifest
            .save(ctx.clone())
            .wait()
            .expect("Could not save manifest");
        let manifest_id = HgManifestId::new(manifest_entry.get_hash().into_nodehash());

        let refound = repo
            .get_manifest_by_nodeid(ctx.clone(), manifest_id)
            .map(|m| m.lookup(&path))
            .wait()
            .expect("Lookup of entry just saved failed")
            .expect("Just saved entry not present");

        // Confirm that the entry we put in the root manifest is present
        assert_eq!(
            refound.get_hash().into_nodehash(),
            dir_nodehash,
            "directory hash changed"
        );
    })
}

#[test]
fn remove_item() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let repo = many_files_dirs::getrepo(None);
        let blobstore = repo.get_blobstore();

        let manifest_id = HgNodeHash::from_static_str("907f5b20e06dfb91057861d984423e84b64b5b7b")
            .expect("Could not get nodehash");

        let dir2 = MPathElement::new(b"dir2".to_vec()).expect("Can't create MPathElement dir2");

        // Load a memory manifest
        let memory_manifest = MemoryRootManifest::new(
            ctx.clone(),
            repo.clone(),
            IncompleteFilenodes::new(),
            Some(manifest_id),
            None,
        )
        .wait()
        .expect("Could not load manifest");

        if !memory_manifest.unittest_root().is_dir() {
            panic!("Loaded manifest is not a MemTree");
        }

        // Remove a file
        memory_manifest
            .change_entry(
                ctx.clone(),
                &MPath::new(b"dir2/file_1_in_dir2").expect("Can't create MPath"),
                None,
            )
            .wait()
            .expect("Failed to remove");

        // Assert that dir2 is now empty, since we've removed the item
        if let MemoryManifestEntry::MemTree { ref changes, .. } = memory_manifest.unittest_root() {
            let changes = changes.lock().expect("lock poisoned");
            assert!(
                changes
                    .get(&dir2)
                    .expect("dir2 is missing")
                    .clone()
                    .map_or(false, |e| e
                        .is_empty(ctx.clone(), &blobstore)
                        .wait()
                        .unwrap()),
                "Bad after remove"
            );
            if let Some(MemoryManifestEntry::MemTree { changes, .. }) =
                changes.get(&dir2).expect("dir2 is missing")
            {
                let changes = changes.lock().expect("lock poisoned");
                assert!(!changes.is_empty(), "dir2 has no change entries");
                assert!(
                    changes.values().all(Option::is_none),
                    "dir2 has some add entries"
                );
            }
        } else {
            panic!("Loaded manifest is not a MemTree");
        }

        // And check that dir2 disappears over a save/reload operation
        let manifest_entry = memory_manifest
            .save(ctx.clone())
            .wait()
            .expect("Could not save manifest");
        let manifest_id = HgManifestId::new(manifest_entry.get_hash().into_nodehash());

        let refound = repo
            .get_manifest_by_nodeid(ctx.clone(), manifest_id)
            .map(|m| m.lookup(&dir2))
            .wait()
            .expect("Lookup of entry just saved failed");

        assert!(
            refound.is_none(),
            "Found dir2 when we should have deleted it on save"
        );
    })
}

#[test]
fn add_item() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let repo = many_files_dirs::getrepo(None);
        let blobstore = repo.get_blobstore();

        let manifest_id = HgNodeHash::from_static_str("907f5b20e06dfb91057861d984423e84b64b5b7b")
            .expect("Could not get nodehash");

        let new_file =
            MPathElement::new(b"new_file".to_vec()).expect("Can't create MPathElement new_file");

        // Load a memory manifest
        let memory_manifest = MemoryRootManifest::new(
            ctx.clone(),
            repo.clone(),
            IncompleteFilenodes::new(),
            Some(manifest_id),
            None,
        )
        .wait()
        .expect("Could not load manifest");

        // Add a file
        let nodehash = HgNodeHash::from_static_str("907f5b20e06dfb91057861d984423e84b64b5b7b")
            .expect("Could not get nodehash");
        memory_manifest
            .change_entry(
                ctx.clone(),
                &MPath::new(b"new_file").expect("Could not create MPath"),
                Some(HgBlobEntry::new(
                    blobstore.clone(),
                    new_file.clone(),
                    nodehash,
                    Type::File(FileType::Regular),
                )),
            )
            .wait()
            .expect("Failed to set");

        // And check that new_file persists
        let manifest_entry = memory_manifest
            .save(ctx.clone())
            .wait()
            .expect("Could not save manifest");
        let manifest_id = HgManifestId::new(manifest_entry.get_hash().into_nodehash());

        let refound = repo
            .get_manifest_by_nodeid(ctx.clone(), manifest_id)
            .map(|m| m.lookup(&new_file))
            .wait()
            .expect("Lookup of entry just saved failed")
            .expect("new_file did not persist");
        assert_eq!(
            refound.get_hash().into_nodehash(),
            nodehash,
            "nodehash hash changed"
        );
    })
}

#[test]
fn replace_item() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let repo = many_files_dirs::getrepo(None);
        let blobstore = repo.get_blobstore();

        let manifest_id = HgNodeHash::from_static_str("907f5b20e06dfb91057861d984423e84b64b5b7b")
            .expect("Could not get nodehash");

        let new_file = MPathElement::new(b"1".to_vec()).expect("Can't create MPathElement 1");

        // Load a memory manifest
        let memory_manifest = MemoryRootManifest::new(
            ctx.clone(),
            repo.clone(),
            IncompleteFilenodes::new(),
            Some(manifest_id),
            None,
        )
        .wait()
        .expect("Could not load manifest");

        // Add a file
        let nodehash = HgNodeHash::from_static_str("907f5b20e06dfb91057861d984423e84b64b5b7b")
            .expect("Could not get nodehash");
        memory_manifest
            .change_entry(
                ctx.clone(),
                &MPath::new(b"1").expect("Could not create MPath"),
                Some(HgBlobEntry::new(
                    blobstore.clone(),
                    new_file.clone(),
                    nodehash,
                    Type::File(FileType::Regular),
                )),
            )
            .wait()
            .expect("Failed to set");

        // And check that new_file persists
        let manifest_entry = memory_manifest
            .save(ctx.clone())
            .wait()
            .expect("Could not save manifest");
        let manifest_id = HgManifestId::new(manifest_entry.get_hash().into_nodehash());

        let refound = repo
            .get_manifest_by_nodeid(ctx, manifest_id)
            .map(|m| m.lookup(&new_file))
            .wait()
            .expect("Lookup of entry just saved failed")
            .expect("1 did not persist");
        assert_eq!(
            refound.get_hash().into_nodehash(),
            nodehash,
            "nodehash hash changed"
        );
    })
}

#[test]
fn conflict_resolution() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let repo = many_files_dirs::getrepo(None);
        let blobstore = repo.get_blobstore();
        let logger = repo.get_logger();

        let dir_file_conflict = MPathElement::new(b"dir_file_conflict".to_vec()).unwrap();

        let base = {
            let mut changes = BTreeMap::new();

            changes.insert(
                dir_file_conflict.clone(),
                Some(MemoryManifestEntry::Blob(HgBlobEntry::new(
                    blobstore.clone(),
                    dir_file_conflict.clone(),
                    nodehash::ONES_HASH,
                    Type::File(FileType::Regular),
                ))),
            );
            MemoryManifestEntry::MemTree {
                base_manifest_id: None,
                p1: Some(nodehash::ONES_HASH),
                p2: None,
                changes: Arc::new(Mutex::new(changes)),
            }
        };

        let other = {
            let mut changes = BTreeMap::new();

            let other_sub = {
                let mut changes = BTreeMap::new();
                let file = MPathElement::new(b"file".to_vec()).unwrap();
                changes.insert(
                    file.clone(),
                    Some(MemoryManifestEntry::Blob(HgBlobEntry::new(
                        blobstore.clone(),
                        file.clone(),
                        nodehash::ONES_HASH,
                        Type::File(FileType::Regular),
                    ))),
                );
                MemoryManifestEntry::MemTree {
                    base_manifest_id: None,
                    p1: None,
                    p2: None,
                    changes: Arc::new(Mutex::new(changes)),
                }
            };
            changes.insert(dir_file_conflict.clone(), Some(other_sub));

            MemoryManifestEntry::MemTree {
                base_manifest_id: None,
                p1: Some(nodehash::ONES_HASH),
                p2: None,
                changes: Arc::new(Mutex::new(changes)),
            }
        };

        let merge = run_future(base.merge_with_conflicts(
            ctx,
            other,
            blobstore,
            logger,
            IncompleteFilenodes::new(),
            RepoPath::root(),
        ))
        .unwrap();
        match &merge {
            MemoryManifestEntry::MemTree { changes, .. } => {
                let changes = changes.lock().expect("lock poisoned");
                match changes.get(&dir_file_conflict) {
                    Some(Some(MemoryManifestEntry::Conflict(conflict))) => {
                        assert_eq!(conflict.len(), 2)
                    }
                    _ => panic!("Conflict expected"),
                }
            }
            _ => panic!("Tree expected"),
        };

        merge
            .change(dir_file_conflict.clone(), None)
            .expect("Should succeed");
        match &merge {
            MemoryManifestEntry::MemTree { changes, .. } => {
                let changes = changes.lock().expect("lock poisoned");
                match changes.get(&dir_file_conflict) {
                    Some(Some(MemoryManifestEntry::MemTree { .. })) => (),
                    _ => panic!("Tree expected"),
                }
            }
            _ => panic!("Tree expected"),
        };
    });
}

#[test]
fn merge_manifests() {
    async_unit::tokio_unit_test(|| {
        let ctx = CoreContext::test_mock();
        let repo = many_files_dirs::getrepo(None);
        let blobstore = repo.get_blobstore();
        let logger = repo.get_logger();

        let base = {
            let mut changes = BTreeMap::new();
            let shared = MPathElement::new(b"shared".to_vec()).unwrap();
            let base = MPathElement::new(b"base".to_vec()).unwrap();
            let conflict = MPathElement::new(b"conflict".to_vec()).unwrap();
            changes.insert(
                shared.clone(),
                Some(MemoryManifestEntry::Blob(HgBlobEntry::new(
                    blobstore.clone(),
                    shared.clone(),
                    nodehash::ONES_HASH,
                    Type::File(FileType::Regular),
                ))),
            );
            changes.insert(
                base.clone(),
                Some(MemoryManifestEntry::Blob(HgBlobEntry::new(
                    blobstore.clone(),
                    base.clone(),
                    nodehash::ONES_HASH,
                    Type::File(FileType::Regular),
                ))),
            );
            changes.insert(
                conflict.clone(),
                Some(MemoryManifestEntry::Blob(HgBlobEntry::new(
                    blobstore.clone(),
                    conflict.clone(),
                    nodehash::ONES_HASH,
                    Type::File(FileType::Regular),
                ))),
            );
            MemoryManifestEntry::MemTree {
                base_manifest_id: None,
                p1: Some(nodehash::ONES_HASH),
                p2: None,
                changes: Arc::new(Mutex::new(changes)),
            }
        };

        let other = {
            let mut changes = BTreeMap::new();
            let shared = MPathElement::new(b"shared".to_vec()).unwrap();
            let other = MPathElement::new(b"other".to_vec()).unwrap();
            let conflict = MPathElement::new(b"conflict".to_vec()).unwrap();
            changes.insert(
                shared.clone(),
                Some(MemoryManifestEntry::Blob(HgBlobEntry::new(
                    blobstore.clone(),
                    shared.clone(),
                    nodehash::ONES_HASH,
                    Type::File(FileType::Regular),
                ))),
            );
            changes.insert(
                other.clone(),
                Some(MemoryManifestEntry::Blob(HgBlobEntry::new(
                    blobstore.clone(),
                    other.clone(),
                    nodehash::TWOS_HASH,
                    Type::File(FileType::Regular),
                ))),
            );
            changes.insert(
                conflict.clone(),
                Some(MemoryManifestEntry::Blob(HgBlobEntry::new(
                    blobstore.clone(),
                    conflict.clone(),
                    nodehash::TWOS_HASH,
                    Type::File(FileType::Regular),
                ))),
            );
            MemoryManifestEntry::MemTree {
                base_manifest_id: None,
                p1: Some(nodehash::TWOS_HASH),
                p2: None,
                changes: Arc::new(Mutex::new(changes)),
            }
        };

        let merged = base
            .merge_with_conflicts(
                ctx,
                other,
                blobstore,
                logger,
                IncompleteFilenodes::new(),
                RepoPath::root(),
            )
            .wait()
            .unwrap();

        if let MemoryManifestEntry::MemTree { changes, .. } = merged {
            let changes = changes.lock().expect("lock poisoned");
            assert_eq!(changes.len(), 4, "Should merge to 4 entries");
            if let Some(Some(MemoryManifestEntry::Blob(blob))) =
                changes.get(&MPathElement::new(b"shared".to_vec()).unwrap())
            {
                assert_eq!(
                    blob.get_hash(),
                    (FileType::Regular, HgFileNodeId::new(nodehash::ONES_HASH)).into(),
                    "Wrong hash for shared"
                );
            } else {
                panic!("shared is not a blob");
            }
            if let Some(Some(MemoryManifestEntry::Blob(blob))) =
                changes.get(&MPathElement::new(b"base".to_vec()).unwrap())
            {
                assert_eq!(
                    blob.get_hash(),
                    (FileType::Regular, HgFileNodeId::new(nodehash::ONES_HASH)).into(),
                    "Wrong hash for base"
                );
            } else {
                panic!("base is not a blob");
            }
            if let Some(Some(MemoryManifestEntry::Blob(blob))) =
                changes.get(&MPathElement::new(b"other".to_vec()).unwrap())
            {
                assert_eq!(
                    blob.get_hash(),
                    (FileType::Regular, HgFileNodeId::new(nodehash::TWOS_HASH)).into(),
                    "Wrong hash for other"
                );
            } else {
                panic!("other is not a blob");
            }
            if let Some(Some(MemoryManifestEntry::Conflict(conflicts))) =
                changes.get(&MPathElement::new(b"conflict".to_vec()).unwrap())
            {
                assert_eq!(conflicts.len(), 2, "Should have two conflicts");
            } else {
                panic!("conflict did not create a conflict")
            }
        } else {
            panic!("Merge failed to produce a merged tree");
        }
    })
}

#[test]
fn save_reproducibility_under_load() -> Result<()> {
    let ctx = CoreContext::test_mock();
    let delay_settings = DelaySettings {
        blobstore_put_dist: Normal::new(0.01, 0.005),
        blobstore_get_dist: Normal::new(0.005, 0.0025),
        db_put_dist: Normal::new(0.002, 0.001),
        db_get_dist: Normal::new(0.002, 0.001),
    };
    cmdlib::args::init_cachelib_from_settings(Default::default())?;
    let repo = new_benchmark_repo(delay_settings)?;

    let mut rng = XorShiftRng::seed_from_u64(1);
    let mut gen = GenManifest::new();
    let settings = Default::default();

    let test = gen
        .gen_stack(
            ctx.clone(),
            repo.clone(),
            &mut rng,
            &settings,
            None,
            std::iter::repeat(16).take(50),
        )
        .and_then(move |csid| repo.get_hg_from_bonsai_changeset(ctx, csid));

    let mut runtime = Runtime::new()?;
    assert_eq!(
        runtime.block_on(test)?,
        "6f67e722196896e645eec15b1d40fb0ecc5488d6".parse()?,
    );

    Ok(())
}
