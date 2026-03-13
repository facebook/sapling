/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use cacheblob::MemWritesKeyedBlobstore;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::override_just_knobs;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_derivation::RootHgAugmentedManifestId;
use mercurial_derivation::derive_hg_augmented_manifest;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgManifestId;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use restricted_paths::RestrictedPathsRef;
use tests_utils::CreateCommitContext;
use tests_utils::drawdag::extend_from_dag_with_actions;

use crate::Repo;

/// Invariant test: after augmented manifest derivation (which inline-derives
/// HgChangesets), all HgManifest blobs must be loadable via `Loadable`.
#[mononoke::fbinit_test]
async fn test_augmented_manifest_hg_blobs_loadable(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir_a/file", "1")
        .add_file("dir_b/file", "2")
        .commit()
        .await?;
    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("dir_a/file", "3")
        .add_file("dir_c/file", "4")
        .commit()
        .await?;

    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, vec![root, child], None)
        .await?;

    // Verify ALL HgManifest blobs are loadable (root + subdirectory manifests).
    for cs_id in [root, child] {
        let hg_cs_id = repo.derive_hg_changeset(&ctx, cs_id).await?;
        let hg_mf_id = hg_cs_id
            .load(&ctx, repo.repo_blobstore())
            .await?
            .manifestid();
        // Load root manifest via Loadable
        let _root_mf = hg_mf_id.load(&ctx, repo.repo_blobstore()).await?;
        // Load all subtree manifests via list_all_entries
        let entries: Vec<_> = hg_mf_id
            .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
            .try_collect()
            .await?;
        // Sanity: should have both files and directories
        assert!(!entries.is_empty());
    }
    Ok(())
}

async fn get_manifests(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    parents: Vec<HgAugmentedManifestId>,
) -> Result<(HgManifestId, HgAugmentedManifestId)> {
    let hg_id = repo
        .derive_hg_changeset(ctx, cs_id)
        .await?
        .load(ctx, repo.repo_blobstore())
        .await?
        .manifestid();

    // First derive the manifest in full using a temporary side blobstore.
    let blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let full_aug_id = derive_hg_augmented_manifest::derive_from_full_hg_manifest(
        ctx.clone(),
        blobstore.clone(),
        hg_id,
    )
    .await?;
    let full_aug = full_aug_id.load(ctx, &blobstore).await?;

    let restricted_paths_config = repo.restricted_paths().config_based();
    // Now derive the manifest using the parents in the main blobstore.
    let aug_id = derive_hg_augmented_manifest::derive_from_hg_manifest_and_parents(
        ctx,
        repo.repo_blobstore(),
        hg_id,
        parents,
        &Default::default(),
        restricted_paths_config,
    )
    .await?;
    let aug = aug_id.load(ctx, repo.repo_blobstore()).await?;

    // Check that the two manifests are the same.
    assert_eq!(aug, full_aug);

    Ok((hg_id, aug_id))
}

async fn compare_manifests(
    ctx: &CoreContext,
    repo: &Repo,
    hg_id: HgManifestId,
    aug_id: HgAugmentedManifestId,
) -> Result<()> {
    let mut hg_e_entries: Vec<_> = hg_id
        .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
        .try_collect()
        .await?;
    let mut aug_e_entries: Vec<_> = aug_id
        .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
        .try_collect()
        .await?;

    hg_e_entries.sort_by_key(|(path, _)| path.clone());
    aug_e_entries.sort_by_key(|(path, _)| path.clone());

    assert_eq!(hg_e_entries.len(), aug_e_entries.len());
    for ((hg_path, hg_entry), (aug_path, aug_entry)) in
        hg_e_entries.iter().zip(aug_e_entries.iter())
    {
        assert_eq!(hg_path, aug_path);
        match (hg_entry, aug_entry) {
            (Entry::Tree(hg_tree), Entry::Tree(aug_tree)) => {
                assert_eq!(hg_tree.into_nodehash(), aug_tree.into_nodehash());
            }
            (Entry::Leaf((file_type, filenode)), Entry::Leaf(aug_leaf)) => {
                assert_eq!(file_type, &aug_leaf.file_type);
                assert_eq!(filenode.into_nodehash(), aug_leaf.filenode);
            }
            _ => {
                panic!(
                    "Mismatched entry types for {}: {:?} vs {:?}",
                    hg_path, hg_entry, aug_entry
                );
            }
        }
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    let (commits, _dag) = extend_from_dag_with_actions(
        &ctx,
        &repo,
        r#"
            A-B-C-E
             \   /
              -D-
            # default_files: false
            # modify: A animals "0"
            # modify: A black/tiger "1"
            # modify: A black/tortoise "2"
            # modify: A black/turtle "3"
            # modify: A black/falcon "4"
            # modify: A black/fox "5"
            # modify: A black/horse "6"
            # modify: A blue/ostrich "7"
            # modify: A blue/owl "8"
            # modify: A blue/penguin "9"
            # modify: A blue/rabbit "10"
            # modify: A blue/snake "11"
            # modify: A blue/whale "12"
            # modify: A brown/emu "13"
            # modify: A brown/iguana "14"
            # modify: A brown/koala "15"
            # modify: A brown/llama "16"
            # modify: A brown/panda "17"
            # modify: A brown/rhino "18"
            # modify: A brown/sloth "19"
            # modify: A brown/tiger "20"
            # modify: A orange/cat "21"
            # modify: A orange/dog "22"
            # modify: A orange/fish "23"
            # modify: A orange/giraffe "24"
            # modify: A orange/caterpillar "25"
            # modify: B black/turtle "26"
            # modify: B blue/owl "27"
            # modify: B blue/zebra "28"
            # modify: B orange/caterpillar "29"
            # delete: B black/tortoise
            # modify: C black/tiger "30"
            # delete: C brown/iguana
            # delete: C brown/koala
            # delete: C brown/llama
            # delete: C brown/panda
            # delete: C brown/rhino
            # delete: C brown/sloth
            # delete: C brown/tiger
            # modify: D red/albatross "30"
            # modify: D red/crow "31"
            # modify: D red/eagle "32"
            # modify: D black/falcon "33"
            # modify: E orange/caterpillar "29"
            # modify: E blue/owl "8"
            # modify: E blue/zebra "31"
            # modify: E black/falcon "33"
            # modify: E black/tiger "1"
            # delete: E black/turtle
            # delete: E black/tortoise
        "#,
    )
    .await?;

    let (hg_a, aug_a) = get_manifests(&ctx, &repo, commits["A"], vec![]).await?;
    let (hg_b, aug_b) = get_manifests(&ctx, &repo, commits["B"], vec![aug_a]).await?;
    let (hg_c, aug_c) = get_manifests(&ctx, &repo, commits["C"], vec![aug_b]).await?;
    let (hg_d, aug_d) = get_manifests(&ctx, &repo, commits["D"], vec![aug_a]).await?;
    let (hg_e, aug_e) = get_manifests(&ctx, &repo, commits["E"], vec![aug_c, aug_d]).await?;

    compare_manifests(&ctx, &repo, hg_a, aug_a).await?;
    compare_manifests(&ctx, &repo, hg_b, aug_b).await?;
    compare_manifests(&ctx, &repo, hg_c, aug_c).await?;
    compare_manifests(&ctx, &repo, hg_d, aug_d).await?;
    compare_manifests(&ctx, &repo, hg_e, aug_e).await?;

    Ok(())
}

/// Test that RootHgAugmentedManifestId batch derivation works correctly
/// across multiple batch segments. This exercises the scenario where
/// derive_heads_with_visited splits a large set of commits into multiple
/// calls to derive_exactly_batch.
///
/// RootHgAugmentedManifestId's derive_batch derives HgChangesets inline
/// and stores their mappings to SQL. The second batch segment depends on
/// HgChangeset mappings from the first batch being visible via SQL.
#[mononoke::fbinit_test]
async fn test_augmented_manifest_multi_batch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    // Create a linear chain of commits. We'll split them into two batch
    // segments to simulate what derive_heads_with_visited does.
    let mut csids = Vec::new();
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file", "content_0")
        .commit()
        .await?;
    csids.push(root);

    for i in 1..10 {
        let parent = *csids.last().unwrap();
        let cs = CreateCommitContext::new(&ctx, &repo, vec![parent])
            .add_file("file", format!("content_{}", i))
            .commit()
            .await?;
        csids.push(cs);
    }

    let manager = repo.repo_derived_data().manager();

    // MappedHgChangesetId is NOT a dependency — it gets derived inline
    // within derive_batch and stored to SQL for cross-batch visibility.
    // Split into two batches and derive RootHgAugmentedManifestId.
    // First batch: commits 0..5
    let batch1 = csids[0..5].to_vec();
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, batch1, None)
        .await?;

    // Second batch: commits 5..10 (parent of commit 5 is commit 4, which
    // was in batch 1). This works because derive_batch stores
    // MappedHgChangesetId mappings to SQL, making them visible here.
    let batch2 = csids[5..10].to_vec();
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, batch2, None)
        .await?;

    // Verify all derived augmented manifests match full derivation
    let derived = manager
        .fetch_derived_batch::<RootHgAugmentedManifestId>(&ctx, csids.clone(), None)
        .await?;

    for cs_id in &csids {
        let aug_id = derived
            .get(cs_id)
            .unwrap_or_else(|| panic!("Missing RootHgAugmentedManifestId for {}", cs_id))
            .hg_augmented_manifest_id();

        let hg_cs_id = repo.derive_hg_changeset(&ctx, *cs_id).await?;
        let hg_manifest_id = hg_cs_id
            .load(&ctx, repo.repo_blobstore())
            .await?
            .manifestid();

        compare_manifests(&ctx, &repo, hg_manifest_id, aug_id).await?;
    }

    Ok(())
}

/// Test that RootHgAugmentedManifestId derivation via derive_heads works
/// correctly when MappedHgChangesetId is not a declared dependency.
/// derive_heads calls derive_exactly_batch, which calls our derive_batch
/// override that inline-derives HgChangesets and stores their mappings
/// after flushing blobs.
#[mononoke::fbinit_test]
async fn test_augmented_manifest_derive_heads(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    // Create a linear chain of 3 commits. derive_heads will process
    // these via derive_exactly_batch -> derive_batch.
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file", "root")
        .commit()
        .await?;

    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("file", "child")
        .commit()
        .await?;

    let grandchild = CreateCommitContext::new(&ctx, &repo, vec![child])
        .add_file("file", "grandchild")
        .commit()
        .await?;

    let manager = repo.repo_derived_data().manager();

    // Do NOT pre-derive MappedHgChangesetId. derive_batch handles
    // inline derivation and mapping persistence.
    manager
        .derive_heads::<RootHgAugmentedManifestId>(ctx.clone(), vec![grandchild], None, None)
        .await?;

    // Verify all augmented manifests were derived correctly.
    for cs_id in [root, child, grandchild] {
        let aug = manager
            .fetch_derived::<RootHgAugmentedManifestId>(&ctx, cs_id, None)
            .await?
            .unwrap_or_else(|| panic!("Missing RootHgAugmentedManifestId for {}", cs_id));

        let hg_cs_id = repo.derive_hg_changeset(&ctx, cs_id).await?;
        let hg_manifest_id = hg_cs_id
            .load(&ctx, repo.repo_blobstore())
            .await?
            .manifestid();

        compare_manifests(&ctx, &repo, hg_manifest_id, aug.hg_augmented_manifest_id()).await?;
    }

    Ok(())
}

/// Test that hgmanifest_skip_writes=true prevents HgManifest blobs from
/// being written to the blobstore, while the reconstruction layer in
/// fetch_manifest_envelope_opt still makes them loadable.
#[mononoke::fbinit_test]
async fn test_augmented_manifest_skip_writes(fb: FacebookInit) -> Result<()> {
    override_just_knobs(JustKnobsInMemory::new(HashMap::from([(
        "scm/mononoke:hgmanifest_skip_writes".to_string(),
        KnobVal::Bool(true),
    )])));

    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/file_a", "content_a")
        .add_file("dir/file_b", "content_b")
        .add_file("other/file_c", "content_c")
        .commit()
        .await?;

    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("dir/file_a", "updated_a")
        .add_file("new/file_d", "content_d")
        .commit()
        .await?;

    let grandchild = CreateCommitContext::new(&ctx, &repo, vec![child])
        .add_file("other/file_c", "updated_c")
        .commit()
        .await?;

    let manager = repo.repo_derived_data().manager();

    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(
            &ctx,
            vec![root, child, grandchild],
            None,
        )
        .await?;

    for cs_id in [root, child, grandchild] {
        let aug = manager
            .fetch_derived::<RootHgAugmentedManifestId>(&ctx, cs_id, None)
            .await?
            .unwrap_or_else(|| panic!("Missing RootHgAugmentedManifestId for {}", cs_id));

        let hg_cs_id = repo.derive_hg_changeset(&ctx, cs_id).await?;
        let hg_manifest_id = hg_cs_id
            .load(&ctx, repo.repo_blobstore())
            .await?
            .manifestid();

        // HgManifest blobs are loadable via the reconstruction layer.
        let _root_mf = hg_manifest_id.load(&ctx, repo.repo_blobstore()).await?;

        let entries: Vec<_> = hg_manifest_id
            .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
            .try_collect()
            .await?;
        assert!(!entries.is_empty());

        compare_manifests(&ctx, &repo, hg_manifest_id, aug.hg_augmented_manifest_id()).await?;

        // Verify HgManifest blobs were NOT written to the blobstore.
        // Raw blobstore.get() bypasses the reconstruction layer in
        // fetch_manifest_envelope_opt.
        let blobstore = repo.repo_blobstore();
        assert!(
            blobstore
                .get(&ctx, &hg_manifest_id.blobstore_key())
                .await?
                .is_none(),
            "Root HgManifest blob should not exist in blobstore when skip_writes is enabled",
        );
        for entry in &entries {
            if let (path, Entry::Tree(subtree_mf_id)) = entry {
                assert!(
                    blobstore
                        .get(&ctx, &subtree_mf_id.blobstore_key())
                        .await?
                        .is_none(),
                    "HgManifest blob for subtree {:?} should not exist in blobstore",
                    path,
                );
            }
        }
    }

    Ok(())
}
