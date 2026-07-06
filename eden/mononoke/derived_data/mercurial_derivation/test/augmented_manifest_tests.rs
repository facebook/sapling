/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use acl_manifest::RootAclManifestId;
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
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
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
