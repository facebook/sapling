/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use acl_manifest::RootAclManifestId;
use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use cacheblob::MemWritesKeyedBlobstore;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_derivation::RootHgAugmentedManifestId;
use mercurial_derivation::derive_hg_augmented_manifest;
use mercurial_types::HgAugmentedManifestEnvelope;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgManifestId;
use mercurial_types::HgParents;
use mercurial_types_mocks::nodehash::AS_HASH;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use restricted_paths::RestrictedPathsRef;
use tests_utils::CreateCommitContext;
use tests_utils::drawdag::extend_from_dag_with_actions;

use crate::Repo;

/// Invariant test: after augmented manifest derivation, all HgManifest
/// blobs must be loadable via `Loadable`.
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

    // HgChangesets must be derived first (dependency of RootHgAugmentedManifestId).
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, vec![root, child], None)
        .await?;

    // Pre-derive RootAclManifestId (batch dependency of RootHgAugmentedManifestId)
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![root, child], None)
        .await?;

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

    // Derive ACL manifest for this changeset to get the overlay.
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(ctx, vec![cs_id], None)
        .await?;
    let acl_root = manager
        .fetch_derived::<RootAclManifestId>(ctx, cs_id, None)
        .await?
        .unwrap_or_else(|| panic!("Missing RootAclManifestId for {cs_id}"));
    let acl_root_overlay = derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;

    // First derive the manifest in full using a temporary side blobstore.
    let blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let full_aug_id = derive_hg_augmented_manifest::derive_from_full_hg_manifest(
        ctx.clone(),
        blobstore.clone(),
        hg_id,
        acl_root_overlay,
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
        acl_root_overlay,
    )
    .await?;
    let aug = aug_id.load(ctx, repo.repo_blobstore()).await?;

    // Verify ACL pointers match between full and parent-aware derivation
    assert_eq!(
        aug.augmented_manifest.acl_manifest_directory_id,
        full_aug.augmented_manifest.acl_manifest_directory_id,
        "acl_manifest_directory_id mismatch at root between full and parent-aware derivation"
    );

    // Check that the two manifests are the same (deep equality includes ACL pointers).
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
                panic!("Mismatched entry types for {hg_path}: {hg_entry:?} vs {aug_entry:?}");
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
            .add_file("file", format!("content_{i}"))
            .commit()
            .await?;
        csids.push(cs);
    }

    let manager = repo.repo_derived_data().manager();

    // Pre-derive dependencies of RootHgAugmentedManifestId.
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, csids.clone(), None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
        .await?;

    // Split into two batches and derive RootHgAugmentedManifestId.
    // First batch: commits 0..5
    let batch1 = csids[0..5].to_vec();
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, batch1, None)
        .await?;

    // Second batch: commits 5..10 (parent of commit 5 is commit 4, which
    // was in batch 1).
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
            .unwrap_or_else(|| panic!("Missing RootHgAugmentedManifestId for {cs_id}"))
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
            .unwrap_or_else(|| panic!("Missing RootHgAugmentedManifestId for {cs_id}"));

        let hg_cs_id = repo.derive_hg_changeset(&ctx, cs_id).await?;
        let hg_manifest_id = hg_cs_id
            .load(&ctx, repo.repo_blobstore())
            .await?
            .manifestid();

        compare_manifests(&ctx, &repo, hg_manifest_id, aug.hg_augmented_manifest_id()).await?;
    }

    Ok(())
}

/// Test that augmented manifest derivation produces identical results
/// from both the parent-aware and full derivation paths when the repo
/// has .slacl files (non-empty AclManifest).
#[mononoke::fbinit_test]
async fn test_augmented_manifest_parity_with_slacl(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    // Root commit with a .slacl file creating a restriction root.
    // ACL tree: root (waypoint) -> restricted (waypoint) -> code (restriction root)
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "restricted/code/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n",
        )
        .add_file("restricted/code/secret.rs", "fn secret() {}")
        .add_file("public/readme.md", "hello")
        .commit()
        .await?;

    // Derive and compare via the parity helper — this should verify that
    // both derivation paths produce identical augmented manifests,
    // including acl_manifest_directory_id pointers.
    let (_, aug_root) = get_manifests(&ctx, &repo, root, vec![]).await?;

    // Child commit adding more files (tests incremental vs full parity
    // when parent manifests exist and subtrees are reused)
    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("restricted/code/more.rs", "fn more() {}")
        .add_file("public/docs.md", "docs")
        .commit()
        .await?;

    let (_, _aug_child) = get_manifests(&ctx, &repo, child, vec![aug_root]).await?;

    Ok(())
}

/// Test that resolve_copy_from_filenodes correctly resolves copy-from
/// source filenodes from parent augmented manifests.
#[mononoke::fbinit_test]
async fn test_resolve_copy_from_filenodes(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    // Create a root commit with a file
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("original_file", "content")
        .commit()
        .await?;

    // Create a child that copies the file
    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file_with_copy_info("copied_file", "content", (root, "original_file"))
        .commit()
        .await?;

    // Derive HgChangesets and augmented manifests for the root (parent)
    let manager = repo.repo_derived_data().manager();

    // HgChangesets must be derived first (dependency of RootHgAugmentedManifestId).
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, vec![root], None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![root], None)
        .await?;

    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, vec![root], None)
        .await?;

    let root_aug = manager
        .fetch_derived::<RootHgAugmentedManifestId>(&ctx, root, None)
        .await?
        .expect("Missing augmented manifest for root")
        .hg_augmented_manifest_id();

    // Build file_changes for the child commit
    let child_bonsai: mononoke_types::BonsaiChangeset =
        child.load(&ctx, repo.repo_blobstore()).await?;
    let file_changes: Vec<_> = child_bonsai
        .file_changes()
        .map(|(path, fc)| {
            let tc = match fc {
                FileChange::Change(tc) => Some(tc.clone()),
                FileChange::Deletion
                | FileChange::UntrackedChange(_)
                | FileChange::UntrackedDeletion => None,
            };
            (path.clone(), tc)
        })
        .collect();

    // Resolve copy-from filenodes
    let result = derive_hg_augmented_manifest::resolve_copy_from_filenodes(
        &ctx,
        repo.repo_blobstore(),
        &file_changes,
        &[Some((root, root_aug)), None],
    )
    .await?;

    // Should have resolved the copy-from for "copied_file" -> "original_file"
    let original_path = NonRootMPath::new("original_file")?;
    assert!(
        result.contains_key(&(original_path.clone(), root)),
        "Should resolve copy-from for original_file in parent {root}",
    );

    // Verify the resolved filenode matches the actual filenode in the parent HgManifest
    let hg_cs_id = repo.derive_hg_changeset(&ctx, root).await?;
    let hg_manifest_id = hg_cs_id
        .load(&ctx, repo.repo_blobstore())
        .await?
        .manifestid();

    let expected_entries: Vec<_> = hg_manifest_id
        .find_entries(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            vec![manifest::PathOrPrefix::Path(original_path.clone().into())],
        )
        .try_collect()
        .await?;

    let expected_filenode = match &expected_entries[0].1 {
        Entry::Leaf((_, filenode)) => *filenode,
        _ => panic!("Expected a leaf entry"),
    };

    assert_eq!(
        result[&(original_path, root)],
        expected_filenode,
        "Resolved filenode should match HgManifest filenode"
    );

    Ok(())
}

/// Test that resolve_copy_from_filenodes skips (rather than errors) when
/// the copy-from source path doesn't exist in the parent manifest.
/// This matches the behavior of the HgManifest-based derivation path in
/// derive_hg_changeset.rs.
#[mononoke::fbinit_test]
async fn test_resolve_copy_from_filenodes_missing_source(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    // Create root with file.txt
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("file.txt", "content")
        .commit()
        .await?;

    // Delete file.txt in the child
    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .delete_file("file.txt")
        .commit()
        .await?;

    // Create a grandchild that claims to copy from file.txt at the child
    // (where file.txt no longer exists)
    let grandchild = CreateCommitContext::new(&ctx, &repo, vec![child])
        .add_file_with_copy_info("new_file", "content", (child, "file.txt"))
        .commit()
        .await?;

    let manager = repo.repo_derived_data().manager();

    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, vec![root, child], None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![root, child], None)
        .await?;
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, vec![root, child], None)
        .await?;

    let child_aug = manager
        .fetch_derived::<RootHgAugmentedManifestId>(&ctx, child, None)
        .await?
        .expect("Missing augmented manifest for child")
        .hg_augmented_manifest_id();

    let grandchild_bonsai: mononoke_types::BonsaiChangeset =
        grandchild.load(&ctx, repo.repo_blobstore()).await?;
    let file_changes: Vec<_> = grandchild_bonsai
        .file_changes()
        .map(|(path, fc)| {
            let tc = match fc {
                FileChange::Change(tc) => Some(tc.clone()),
                FileChange::Deletion
                | FileChange::UntrackedChange(_)
                | FileChange::UntrackedDeletion => None,
            };
            (path.clone(), tc)
        })
        .collect();

    let result = derive_hg_augmented_manifest::resolve_copy_from_filenodes(
        &ctx,
        repo.repo_blobstore(),
        &file_changes,
        &[Some((child, child_aug)), None],
    )
    .await?;

    // The copy-from entry should be absent (skipped) since file.txt
    // doesn't exist in the child's manifest.
    let missing_path = NonRootMPath::new("file.txt")?;
    assert!(
        !result.contains_key(&(missing_path, child)),
        "Should skip copy-from when source path is missing from parent manifest"
    );

    Ok(())
}

/// Verify the streaming `compute_hg_node_id` produces the same hash as the
/// materialising `serialize_manifest` + `calculate_hg_node_id` reference path.
///
/// This is the correctness contract that lets the new direct-derivation path
/// avoid `try_collect`-ing every entry into memory before hashing — a pattern
/// that has caused OOMs on huge directories like `fbcode/third-party`.
#[mononoke::fbinit_test]
async fn test_compute_hg_node_id_matches_materialised(fb: FacebookInit) -> Result<()> {
    use futures::stream::TryStreamExt;
    use mercurial_types::HgAugmentedManifestEntry;
    use mercurial_types::HgParents;
    use mercurial_types::calculate_hg_node_id;
    use mercurial_types::preloaded_augmented_manifest::serialize_manifest;
    use mononoke_types::MPathElement;
    use repo_blobstore::RepoBlobstoreRef;

    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    // Build a real commit so we get a real, populated augmented-manifest envelope.
    let cs_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("alpha", "a")
        .add_file("beta", "b")
        .add_file("subdir/file1", "c")
        .add_file("subdir/file2", "d")
        .add_file("zeta", "e")
        .commit()
        .await?;

    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, vec![cs_id], None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![cs_id], None)
        .await?;
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, vec![cs_id], None)
        .await?;
    let aug_root = manager
        .fetch_derived::<RootHgAugmentedManifestId>(&ctx, cs_id, None)
        .await?
        .expect("derived just above");
    let aug_id = aug_root.hg_augmented_manifest_id();
    let envelope = aug_id.load(&ctx, repo.repo_blobstore()).await?;

    let subentries = envelope.augmented_manifest.subentries.clone();
    let parents = HgParents::new(
        envelope.augmented_manifest.p1,
        envelope.augmented_manifest.p2,
    );

    // Streaming path under test.
    let streaming = derive_hg_augmented_manifest::compute_hg_node_id(
        subentries.clone(),
        &ctx,
        repo.repo_blobstore(),
        &parents,
    )
    .await?;

    // Reference: collect all entries into a Vec, serialise the directory in
    // one go, then hash the assembled bytes via the non-streaming variant.
    let collected: Vec<(MPathElement, HgAugmentedManifestEntry)> = subentries
        .into_entries(&ctx, repo.repo_blobstore())
        .and_then(|(path, entry)| async move { Ok((MPathElement::from_smallvec(path)?, entry)) })
        .try_collect()
        .await?;
    let materialised = serialize_manifest(&collected)?;
    let reference = calculate_hg_node_id(materialised.as_ref(), &parents);

    assert_eq!(
        streaming, reference,
        "streaming compute_hg_node_id must match materialised serialize_manifest + calculate_hg_node_id"
    );
    // And both must equal the canonical hg_node_id stored in the envelope.
    assert_eq!(
        streaming, envelope.augmented_manifest.hg_node_id,
        "streaming compute_hg_node_id must match the canonical hg_node_id stored on the envelope"
    );

    // Cross-check: must also match the hg_node_id that the existing HgManifest
    // derivation path produces for the same commit. This is the contract that
    // lets the new direct-derivation path be a drop-in for HgManifest derivation.
    let hg_cs_id = repo.derive_hg_changeset(&ctx, cs_id).await?;
    let hg_manifest_id = hg_cs_id
        .load(&ctx, repo.repo_blobstore())
        .await?
        .manifestid();
    assert_eq!(
        streaming,
        hg_manifest_id.into_nodehash(),
        "streaming compute_hg_node_id must match HgManifest derivation"
    );

    Ok(())
}

async fn derive_augmented_manifests(
    ctx: &CoreContext,
    repo: &Repo,
    cs_ids: Vec<ChangesetId>,
) -> Result<Vec<(HgAugmentedManifestId, HgAugmentedManifestEnvelope)>> {
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(ctx, cs_ids.clone(), None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(ctx, cs_ids.clone(), None)
        .await?;
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(ctx, cs_ids.clone(), None)
        .await?;

    let mut manifests = Vec::new();
    for cs_id in cs_ids {
        let aug_id = manager
            .fetch_derived::<RootHgAugmentedManifestId>(ctx, cs_id, None)
            .await?
            .unwrap_or_else(|| panic!("Missing RootHgAugmentedManifestId for {cs_id}"))
            .hg_augmented_manifest_id();
        let envelope = aug_id.load(ctx, repo.repo_blobstore()).await?;
        manifests.push((aug_id, envelope));
    }

    Ok(manifests)
}

#[mononoke::fbinit_test]
async fn test_try_reuse_parent_envelope_reuses_matching_parent(fb: FacebookInit) -> Result<()> {
    // Given: merged subentries serialise to the same content as the parent.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1_cs = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("alpha", "1")
        .add_file("beta", "2")
        .add_file("gamma", "3")
        .commit()
        .await?;
    let mut manifests = derive_augmented_manifests(&ctx, &repo, vec![p1_cs]).await?;
    let (p1_aug_id, p1_env) = manifests.pop().expect("derived one augmented manifest");

    // When: probing parent-envelope reuse with the matching parent as p1.
    let reuse = derive_hg_augmented_manifest::try_reuse_parent_envelope(
        &ctx,
        repo.repo_blobstore(),
        p1_env.augmented_manifest.subentries.clone(),
        Some(p1_aug_id),
        None,
    )
    .await?;

    // Then: p1 is reused.
    assert_eq!(
        reuse,
        derive_hg_augmented_manifest::ParentEnvelopeReuse::Reuse(
            derive_hg_augmented_manifest::ReusableParentEnvelope {
                id: p1_aug_id,
                envelope: p1_env,
            }
        ),
        "should reuse the matching parent"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_try_reuse_parent_envelope_creates_fresh_for_different_content(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a candidate parent and merged subentries from unrelated content.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1_cs = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("alpha", "1")
        .add_file("beta", "2")
        .add_file("gamma", "3")
        .commit()
        .await?;
    let other_cs = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("delta", "4")
        .add_file("epsilon", "5")
        .commit()
        .await?;
    let mut manifests = derive_augmented_manifests(&ctx, &repo, vec![p1_cs, other_cs])
        .await?
        .into_iter();
    let (p1_aug_id, _p1_env) = manifests.next().expect("derived p1 augmented manifest");
    let (_other_aug_id, other_env) = manifests.next().expect("derived other augmented manifest");
    let p1_only_parents = HgParents::new(Some(p1_aug_id.into_nodehash()), None);

    // When: probing reuse for subentries that do not match the candidate parent.
    let reuse = derive_hg_augmented_manifest::try_reuse_parent_envelope(
        &ctx,
        repo.repo_blobstore(),
        other_env.augmented_manifest.subentries.clone(),
        Some(p1_aug_id),
        None,
    )
    .await?;

    // Then: no parent is reused and the fresh-envelope hash is returned.
    let expected_fresh_node_id = derive_hg_augmented_manifest::compute_hg_node_id(
        other_env.augmented_manifest.subentries.clone(),
        &ctx,
        repo.repo_blobstore(),
        &p1_only_parents,
    )
    .await?;
    assert_eq!(
        reuse,
        derive_hg_augmented_manifest::ParentEnvelopeReuse::CreateFresh {
            computed_node_id: expected_fresh_node_id,
        },
        "merged subentries differ from parent; expected fresh envelope"
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_try_reuse_parent_envelope_reuses_second_parent_when_first_does_not_match(
    fb: FacebookInit,
) -> Result<()> {
    // Given: two candidate parents where only p2 matches the merged subentries.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1_cs = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("alpha", "1")
        .add_file("beta", "2")
        .add_file("gamma", "3")
        .commit()
        .await?;
    let p2_cs = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("delta", "4")
        .add_file("epsilon", "5")
        .commit()
        .await?;
    let mut manifests = derive_augmented_manifests(&ctx, &repo, vec![p1_cs, p2_cs])
        .await?
        .into_iter();
    let (p1_aug_id, _p1_env) = manifests.next().expect("derived p1 augmented manifest");
    let (p2_aug_id, p2_env) = manifests.next().expect("derived p2 augmented manifest");

    // When: probing reuse for subentries that match only p2.
    let reuse = derive_hg_augmented_manifest::try_reuse_parent_envelope(
        &ctx,
        repo.repo_blobstore(),
        p2_env.augmented_manifest.subentries.clone(),
        Some(p1_aug_id),
        Some(p2_aug_id),
    )
    .await?;

    // Then: p2 is reused.
    assert_eq!(
        reuse,
        derive_hg_augmented_manifest::ParentEnvelopeReuse::Reuse(
            derive_hg_augmented_manifest::ReusableParentEnvelope {
                id: p2_aug_id,
                envelope: p2_env,
            }
        ),
        "should reuse the second parent that matches"
    );

    Ok(())
}

async fn file_changes_from_bonsai(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
) -> Result<Vec<(NonRootMPath, Option<mononoke_types::TrackedFileChange>)>> {
    let bonsai: mononoke_types::BonsaiChangeset = cs_id.load(ctx, repo.repo_blobstore()).await?;
    Ok(bonsai
        .file_changes()
        .map(|(path, file_change)| {
            let tracked_change = match file_change {
                FileChange::Change(tracked_change) => Some(tracked_change.clone()),
                FileChange::Deletion => None,
                _ => None,
            };
            (path.clone(), tracked_change)
        })
        .collect())
}

async fn derive_augmented_manifest_via_existing_path(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    parents: Vec<HgAugmentedManifestId>,
) -> Result<(HgManifestId, HgAugmentedManifestId)> {
    let hg_manifest_id = repo
        .derive_hg_changeset(ctx, cs_id)
        .await?
        .load(ctx, repo.repo_blobstore())
        .await?
        .manifestid();
    let restricted_paths_config = repo.restricted_paths().config_based();
    let augmented_manifest_id = derive_hg_augmented_manifest::derive_from_hg_manifest_and_parents(
        ctx,
        repo.repo_blobstore(),
        hg_manifest_id,
        parents,
        &Default::default(),
        restricted_paths_config,
        None,
    )
    .await?;

    Ok((hg_manifest_id, augmented_manifest_id))
}

async fn assert_direct_derive_matches_existing_path(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    augmented_parents: &[HgAugmentedManifestId],
    bonsai_parents: (Option<ChangesetId>, Option<ChangesetId>),
) -> Result<HgAugmentedManifestId> {
    let (hg_manifest_id, via_existing_path) =
        derive_augmented_manifest_via_existing_path(ctx, repo, cs_id, augmented_parents.to_vec())
            .await?;
    let file_changes = file_changes_from_bonsai(ctx, repo, cs_id).await?;

    let via_direct_path = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        ctx,
        repo.repo_blobstore(),
        augmented_parents.to_vec(),
        file_changes,
        bonsai_parents,
        &Default::default(),
        hg_manifest_id.into_nodehash(),
    )
    .await?;

    assert_eq!(
        via_direct_path, via_existing_path,
        "direct augmented-manifest derivation should match existing HgManifest-based derivation for {cs_id}",
    );

    let direct_envelope = via_direct_path.load(ctx, repo.repo_blobstore()).await?;
    let existing_envelope = via_existing_path.load(ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        direct_envelope.augmented_manifest.hg_node_id,
        existing_envelope.augmented_manifest.hg_node_id,
        "root hg_node_id should match for {cs_id}",
    );
    assert_eq!(
        direct_envelope.augmented_manifest.p1, existing_envelope.augmented_manifest.p1,
        "root p1 should match for {cs_id}",
    );
    assert_eq!(
        direct_envelope.augmented_manifest.p2, existing_envelope.augmented_manifest.p2,
        "root p2 should match for {cs_id}",
    );
    compare_manifests(ctx, repo, hg_manifest_id, via_direct_path).await?;

    Ok(via_direct_path)
}

#[mononoke::fbinit_test]
async fn test_direct_augmented_manifest_matches_existing_path_for_root_commit(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a root commit with both file and directory entries.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let commit = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("README.md", "hello")
        .add_file("src/lib.rs", "pub fn value() -> u8 { 1 }")
        .commit()
        .await?;

    // When: deriving its augmented manifest directly from Bonsai.
    let direct_id =
        assert_direct_derive_matches_existing_path(&ctx, &repo, commit, &[], (None, None)).await?;

    // Then: the direct path produces the same root augmented manifest as the
    // existing HgManifest-based path.
    assert!(
        direct_id.load(&ctx, repo.repo_blobstore()).await.is_ok(),
        "directly derived root envelope should be loadable",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_direct_augmented_manifest_matches_existing_path_for_empty_manifest(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a parent commit with one file and a child commit that deletes it.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let parent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("obsolete", "contents")
        .commit()
        .await?;
    let (_, parent_augmented_manifest_id) =
        derive_augmented_manifest_via_existing_path(&ctx, &repo, parent, vec![]).await?;
    let child = CreateCommitContext::new(&ctx, &repo, vec![parent])
        .delete_file("obsolete")
        .commit()
        .await?;

    // When: deriving the child directly from Bonsai.
    let direct_id = assert_direct_derive_matches_existing_path(
        &ctx,
        &repo,
        child,
        &[parent_augmented_manifest_id],
        (Some(parent), None),
    )
    .await?;

    // Then: the explicit empty-root path still matches the existing derivation.
    let direct_envelope = direct_id.load(&ctx, repo.repo_blobstore()).await?;
    let entries: Vec<_> = direct_envelope
        .augmented_manifest
        .subentries
        .into_entries(&ctx, repo.repo_blobstore())
        .try_collect()
        .await?;
    assert!(entries.is_empty(), "empty manifest should have no entries");

    Ok(())
}

/// Derive the augmented manifest via both paths and assert they agree.
async fn assert_dual_derive_agree(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    aug_parents: &[HgAugmentedManifestId],
    bonsai_parents: (Option<ChangesetId>, Option<ChangesetId>),
) -> Result<HgAugmentedManifestId> {
    assert_direct_derive_matches_existing_path(ctx, repo, cs_id, aug_parents, bonsai_parents).await
}

/// Helper to derive the parent's augmented manifest using the existing path
/// so we have a parent input for the direct-derivation tests.
async fn derive_parent_aug(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
) -> Result<HgAugmentedManifestId> {
    Ok(
        derive_augmented_manifest_via_existing_path(ctx, repo, cs_id, vec![])
            .await?
            .1,
    )
}

/// Assert that a leaves-only conflict rejected by the existing HgManifest
/// derivation is also rejected by direct augmented-manifest derivation.
///
/// This helper intentionally does not use `assert_dual_derive_agree`: in the
/// disagreement case the legacy path should fail before it can provide the
/// canonical root `HgManifestId`. The supplied root hash below is therefore
/// arbitrary; the direct path is expected to fail in the leaf callback before
/// root finalization can use it.
async fn assert_legacy_and_direct_reject_leaf_conflict(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    aug_parents: &[HgAugmentedManifestId],
    bonsai_parents: (Option<ChangesetId>, Option<ChangesetId>),
) -> Result<()> {
    let legacy = repo.derive_hg_changeset(ctx, cs_id).await;
    let legacy_err = legacy.expect_err("legacy HgManifest derivation must reject the conflict");
    let legacy_msg = format!("{legacy_err:#}");
    assert!(
        legacy_msg.contains("Unresolved"),
        "expected legacy unresolved-conflict error, got: {legacy_msg}"
    );

    let file_changes = file_changes_from_bonsai(ctx, repo, cs_id).await?;
    let direct = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        ctx,
        repo.repo_blobstore(),
        aug_parents.to_vec(),
        file_changes,
        bonsai_parents,
        &Default::default(),
        AS_HASH,
    )
    .await;
    assert!(
        direct.is_err(),
        "direct derivation must reject the same leaf conflict as legacy derivation, got: {direct:?}"
    );

    Ok(())
}

/// Octopus merge with three roots that each introduce disjoint top-level
/// files. p1.tree, p2.tree, p3.tree all differ at the root; this is the
/// "ordinary" octopus case that does NOT exercise the positional-indexing
/// regression. It's here as a baseline: if this fails the test scaffolding
/// is broken, not the (X, X, Y) fix.
#[mononoke::fbinit_test]
async fn test_octopus_merge_distinct_parents(fb: FacebookInit) -> Result<()> {
    // Given: three independent parents with disjoint root-level files.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "foo_p1")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("bar", "bar_p2")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("qux", "qux_p3")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;

    // When: creating an octopus merge with no additional file changes.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .commit()
        .await?;

    // Then: the direct derivation agrees with the existing path.
    assert_dual_derive_agree(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3],
        (Some(p1), Some(p2)),
    )
    .await?;

    Ok(())
}

/// Octopus merge where p1.tree == p2.tree but p3.tree differs. This is the
/// shape that reproduces the regression: with positional indexing, the
/// duplicate (p1, p2) survives `derive_manifest`'s value-only dedup as a
/// single entry and the p3 tree slips into the hg-parents window, producing
/// `HgParents::Two(p1, p3)` instead of the correct `HgParents::One(p1)`.
///
/// We reproduce "p1.tree == p2.tree" by giving p1 and p2 *identical content
/// at every path*. That means at the root, and at every subdirectory, the
/// two HgManifestId values are bit-identical. p3 then introduces a top-level
/// file that breaks the equality at the root.
#[mononoke::fbinit_test]
async fn test_octopus_merge_p1_eq_p2_p3_differs(fb: FacebookInit) -> Result<()> {
    // Given: p1 and p2 have identical trees, while p3 has a distinct tree.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared/file", "same_content")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared/file", "same_content")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared/file", "same_content")
        .add_file("extra", "only_in_p3")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;
    assert_eq!(
        aug_p1, aug_p2,
        "Test setup invariant violated: expected p1 and p2 to have identical augmented manifest ids",
    );
    assert_ne!(
        aug_p1, aug_p3,
        "Test setup invariant violated: expected p3 to differ from p1",
    );

    // When: deriving an octopus merge over the (X, X, Y) parents.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .commit()
        .await?;

    // Then: both derivation paths agree despite value-deduped parent entries.
    assert_dual_derive_agree(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3],
        (Some(p1), Some(p2)),
    )
    .await?;

    Ok(())
}

/// Same regression, but at a non-root path. We want to make sure the fix
/// applies to every tree in the recursion, not just the root.
///
/// Setup: all three parents have the same `dir/inner/file`, so
/// `dir/inner` collapses to identical tree ids in p1 and p2; p3 additionally
/// adds a sibling under `dir/inner`, breaking the tree-id equality at the
/// `dir/inner` level. The merge then reads a value at
/// `dir/inner/different` from p3, forcing `dir/inner` to be re-derived.
#[mononoke::fbinit_test]
async fn test_octopus_merge_p1_eq_p2_p3_differs_subdir(fb: FacebookInit) -> Result<()> {
    // Given: the same (X, X, Y) parent shape appears below the root.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/inner/shared", "same")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/inner/shared", "same")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/inner/shared", "same")
        .add_file("dir/inner/different", "p3_only")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;
    assert_eq!(aug_p1, aug_p2, "p1 and p2 must have identical roots");

    // When: deriving an octopus merge that forces the nested directory to be re-derived.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .commit()
        .await?;

    // Then: both derivation paths agree at the root and throughout the tree.
    assert_dual_derive_agree(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3],
        (Some(p1), Some(p2)),
    )
    .await?;

    Ok(())
}

/// Octopus merge where (p2, p3) are equal and p1 differs. Symmetric
/// counterpart: with positional indexing the survivor of dedup is
/// `[Traced(0, X), Traced(1, Y)]` (because the dup is between indices 1
/// and 2, not 0 and 1) so this case happens to compute the *correct*
/// (p1, p2) even with the buggy code. We test it anyway as a regression
/// guard against "fixing" the bug by introducing an *opposite* bug.
#[mononoke::fbinit_test]
async fn test_octopus_merge_p2_eq_p3_p1_differs(fb: FacebookInit) -> Result<()> {
    // Given: p2 and p3 have identical trees while p1 differs.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared/file", "same_content")
        .add_file("extra", "only_in_p1")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared/file", "same_content")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared/file", "same_content")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;
    assert_eq!(aug_p2, aug_p3);

    // When: deriving the symmetric octopus merge.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .commit()
        .await?;

    // Then: both derivation paths agree, guarding against an opposite-direction bug.
    assert_dual_derive_agree(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3],
        (Some(p1), Some(p2)),
    )
    .await?;

    Ok(())
}

/// Octopus merge with leaves-only conflict at p3+: a single file is
/// present at all three parents with identical content but with three
/// different filenodes (because of differing ancestry). `derive_manifest`
/// invokes the leaf callback with `change: None`, and the leaf path used
/// to bail with "only supports two-parent merges". The fix routes the
/// resolution through `hg_parents` and reuses the first hg-relevant
/// parent's leaf, matching the existing HgManifest-based path.
#[mononoke::fbinit_test]
async fn test_octopus_merge_leaves_only_conflict(fb: FacebookInit) -> Result<()> {
    // Given: three parents contain the same file content at `foo`, but each has a distinct filenode.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "same_content")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "same_content")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "same_content")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;

    // When: deriving an octopus merge with no bonsai change for `foo`.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .commit()
        .await?;

    // Then: both paths resolve the leaves-only conflict identically.
    assert_dual_derive_agree(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3],
        (Some(p1), Some(p2)),
    )
    .await?;

    Ok(())
}

/// Octopus leaves-only conflict where p1 and p2 agree on content, but p3
/// carries different content at the same path. The legacy HgManifest resolver
/// checks every deduped parent before choosing a reusable filenode, so this is
/// unresolved; direct derivation must not silently ignore p3 just because p1
/// and p2 are the only Mercurial filenode parents.
#[mononoke::fbinit_test]
async fn test_octopus_merge_hg_parents_agree_step_parent_disagrees(fb: FacebookInit) -> Result<()> {
    // Given: p1 and p2 carry identical `foo` contents, but p3 disagrees.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "same_content")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "same_content")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "different_content")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;

    // When: deriving an octopus merge with no bonsai change for `foo`.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .commit()
        .await?;

    // Then: both derivation paths reject the unresolved leaf conflict.
    assert_legacy_and_direct_reject_leaf_conflict(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3],
        (Some(p1), Some(p2)),
    )
    .await?;

    Ok(())
}

/// Octopus leaves-only conflict where only p1 and p3 carry the path, and they
/// disagree on content. This covers the minimal `(p1, p3)` disagreement shape:
/// the direct path must not reuse p1 merely because p2 is absent.
#[mononoke::fbinit_test]
async fn test_octopus_merge_p1_and_step_parent_disagree(fb: FacebookInit) -> Result<()> {
    // Given: p1 carries `foo`, p2 does not, and p3 carries conflicting `foo` content.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "p1_content")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("bar", "p2_content")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "p3_content")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;

    // When: deriving an octopus merge with no bonsai change for `foo`.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .commit()
        .await?;

    // Then: both derivation paths reject the unresolved leaf conflict.
    assert_legacy_and_direct_reject_leaf_conflict(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3],
        (Some(p1), Some(p2)),
    )
    .await?;

    Ok(())
}

/// Octopus merge that mirrors the existing HgManifest test
/// `derive_hg_manifest test/main.rs::octopus_merges::test_basic_filenode_parents`.
/// Three parents, each contributing one distinct top-level file; the merge
/// modifies all three. This exercises the leaf-side `(p1, p2)` filenode
/// selection (via `hg_parents`) for files that exist only in a single
/// parent: the fix must yield the same filenode parentage as the existing
/// path — in particular `qux` must end up with `(None, None)` because it
/// only existed in p3, which is invisible to Mercurial filenode parentage.
#[mononoke::fbinit_test]
async fn test_octopus_merge_filenode_parents_match_existing_path(fb: FacebookInit) -> Result<()> {
    // Given: three parents each contribute one distinct top-level file.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "foo")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("bar", "bar")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("qux", "qux")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;

    // When: deriving a merge that modifies files from p1, p2, and p3.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .add_file("foo", "foo2")
        .add_file("bar", "bar2")
        .add_file("qux", "qux2")
        .commit()
        .await?;

    // Then: the direct path matches the existing path's filenode-parent choices.
    assert_dual_derive_agree(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3],
        (Some(p1), Some(p2)),
    )
    .await?;

    Ok(())
}

/// Two-parent merge where the two parents have identical trees. This is
/// the boundary case: `derive_manifest`'s value-only dedup collapses
/// `[Traced(0, X), Traced(1, X)]` to `[Traced(0, X)]`. With both positional
/// and index-based filtering this yields `(p1=X, p2=None)`, so this case
/// happens to be safe under both implementations — we test it as a
/// no-regression guard.
#[mononoke::fbinit_test]
async fn test_two_parent_merge_identical_trees(fb: FacebookInit) -> Result<()> {
    // Given: two parents have identical trees.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared/file", "same_content")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("shared/file", "same_content")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    assert_eq!(aug_p1, aug_p2);

    // When: deriving a two-parent merge with no additional changes.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2])
        .commit()
        .await?;

    // Then: both paths agree on the boundary dedup case.
    assert_dual_derive_agree(&ctx, &repo, merge, &[aug_p1, aug_p2], (Some(p1), Some(p2))).await?;

    Ok(())
}

/// Octopus merge where a file (`foo`) lives ONLY in the step-parents (p3/p4),
/// with byte-identical content but different filenodes, and is absent from p1
/// and p2. Both derivation paths must agree, and the only way to agree is to
/// mint a fresh parentless filenode for `foo`: Mercurial filenodes encode only
/// (p1, p2) parentage, so a step-parent's filenode — whose linknode is not a
/// Mercurial ancestor of the merge — cannot be reused.
///
/// p3 and p4 reach `foo = "shared"` from different ancestors (a3/a4), so their
/// `foo` filenodes differ even though the final content is identical. That is
/// what makes this a genuine leaves-only conflict: identical filenodes would
/// otherwise dedup to one entry and never reach the leaf callback.
#[mononoke::fbinit_test]
async fn test_octopus_step_parent_only_file_parity(fb: FacebookInit) -> Result<()> {
    // Given: p1 and p2 do not contain `foo`, while p3 and p4 contain byte-identical `foo` through different filenodes.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("bar", "bar_p1")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("baz", "baz_p2")
        .commit()
        .await?;
    let a3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "ancestor_three")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new(&ctx, &repo, vec![a3])
        .add_file("foo", "shared")
        .commit()
        .await?;
    let a4 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "ancestor_four")
        .commit()
        .await?;
    let p4 = CreateCommitContext::new(&ctx, &repo, vec![a4])
        .add_file("foo", "shared")
        .commit()
        .await?;
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![a3, a4], None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![p1, p2, p3, p4], None)
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;
    let aug_p4 = derive_parent_aug(&ctx, &repo, p4).await?;
    let foo = MPath::new("foo")?;
    let leaf_p3 = aug_p3
        .find_entry(ctx.clone(), repo.repo_blobstore().clone(), foo.clone())
        .await?
        .and_then(Entry::into_leaf)
        .context("p3 must contain foo as a leaf")?;
    let leaf_p4 = aug_p4
        .find_entry(ctx.clone(), repo.repo_blobstore().clone(), foo.clone())
        .await?
        .and_then(Entry::into_leaf)
        .context("p4 must contain foo as a leaf")?;
    assert_ne!(
        leaf_p3.filenode, leaf_p4.filenode,
        "test setup: p3 and p4 must carry foo with DIFFERENT filenodes",
    );
    assert_eq!(
        (leaf_p3.content_sha1, leaf_p3.total_size),
        (leaf_p4.content_sha1, leaf_p4.total_size),
        "test setup: p3 and p4 must carry foo with IDENTICAL content",
    );

    // When: deriving a 4-parent octopus merge with no bonsai change for `foo`.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3, p4])
        .commit()
        .await?;

    // Then: both paths agree, which requires minting the same fresh parentless filenode for `foo`.
    assert_dual_derive_agree(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3, aug_p4],
        (Some(p1), Some(p2)),
    )
    .await?;

    Ok(())
}
