/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use acl_manifest::RootAclManifestId;
use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use cacheblob::MemWritesKeyedBlobstore;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::FutureExt;
use futures::stream::TryStreamExt;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::override_just_knobs;
use justknobs::test_helpers::with_just_knobs_async;
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
use metaconfig_types::PathRestrictionMetadata;
use metaconfig_types::RestrictedPathsConfig;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::RepoPath;
use mononoke_types::SubtreeChange;
use mononoke_types::typed_hash::AclManifestId;
use permission_checker::MononokeIdentity;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use restricted_paths::ManifestType;
use restricted_paths::RestrictedPathManifestIdEntry;
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

    // Pre-derive HgChangesets for the old HgManifest-based augmented path.
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

/// Test that augmented manifest derivation works correctly when
/// hgmanifest_skip_writes=true, verifying that HgManifest blobs remain
/// loadable and augmented manifests match.
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

    // Pre-derive HgChangesets for the old HgManifest-based augmented path.
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, vec![root, child, grandchild], None)
        .await?;

    // Pre-derive RootAclManifestId (batch dependency of RootHgAugmentedManifestId)
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![root, child, grandchild], None)
        .await?;

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
            .unwrap_or_else(|| panic!("Missing RootHgAugmentedManifestId for {cs_id}"));

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

    // Pre-derive HgChangesets for the old HgManifest-based augmented path.
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
        None,
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
        None,
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
        None,
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

#[mononoke::fbinit_test]
async fn test_try_reuse_parent_envelope_returns_none_when_acl_pointer_differs(
    fb: FacebookInit,
) -> Result<()> {
    // Given: merged subentries match the parent content, but `dir_acl_id` differs
    // from the parent's stored ACL pointer. This is a synthetic state — in real
    // derivation content-match implies ACL-match — so we construct it explicitly
    // to lock the `try_reuse_parent_envelope` ACL guard.
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
    let p1_only_parents = HgParents::new(Some(p1_aug_id.into_nodehash()), None);

    let acl_cs = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "dir/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project\"\n",
        )
        .commit()
        .await?;
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![acl_cs], None)
        .await?;
    let some_acl_id: AclManifestId = *manager
        .fetch_derived::<RootAclManifestId>(&ctx, acl_cs, None)
        .await?
        .expect("derived just above")
        .inner_id();

    // When: probing reuse with matching content but a different ACL pointer.
    let no_reuse_acl_mismatch = derive_hg_augmented_manifest::try_reuse_parent_envelope(
        &ctx,
        repo.repo_blobstore(),
        p1_env.augmented_manifest.subentries.clone(),
        Some(some_acl_id),
        Some(p1_aug_id),
        None,
    )
    .await?;

    // Then: no parent is reused and the fresh-envelope hash is returned.
    let expected_fresh_node_id = derive_hg_augmented_manifest::compute_hg_node_id(
        p1_env.augmented_manifest.subentries.clone(),
        &ctx,
        repo.repo_blobstore(),
        &p1_only_parents,
    )
    .await?;
    assert_eq!(
        no_reuse_acl_mismatch,
        derive_hg_augmented_manifest::ParentEnvelopeReuse::CreateFresh {
            computed_node_id: expected_fresh_node_id,
        },
        "content matches but ACL pointer differs (Some vs parent's None); expected fresh envelope"
    );

    Ok(())
}

// -----------------------------------------------------------------------------
// Generic tests for the direct augmented-manifest derivation entry point.
// -----------------------------------------------------------------------------

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

/// Derive `RootAclManifestId` for `cs_id` and normalize it into the
/// `Option<AclManifestId>` overlay shape that the augmented-manifest
/// derivation entry points expect. Idempotent on repeat calls.
async fn derive_acl_overlay(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
) -> Result<Option<AclManifestId>> {
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(ctx, vec![cs_id], None)
        .await?;
    let acl_root = manager
        .fetch_derived::<RootAclManifestId>(ctx, cs_id, None)
        .await?
        .unwrap_or_else(|| panic!("Missing RootAclManifestId for {cs_id}"));
    derive_hg_augmented_manifest::normalize_acl_root(&acl_root)
}

async fn derive_augmented_manifest_via_existing_path(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    parents: Vec<HgAugmentedManifestId>,
    acl_root_overlay: Option<AclManifestId>,
    root_override: Option<HgManifestId>,
) -> Result<(HgManifestId, HgAugmentedManifestId)> {
    let hg_manifest_id = match root_override {
        Some(id) => id,
        None => repo
            .derive_hg_changeset(ctx, cs_id)
            .await?
            .load(ctx, repo.repo_blobstore())
            .await?
            .manifestid(),
    };
    let restricted_paths_config = repo.restricted_paths().config_based();
    let augmented_manifest_id = derive_hg_augmented_manifest::derive_from_hg_manifest_and_parents(
        ctx,
        repo.repo_blobstore(),
        hg_manifest_id,
        parents,
        &Default::default(),
        restricted_paths_config,
        acl_root_overlay,
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
    assert_dual_derive_agree_at_root(ctx, repo, cs_id, augmented_parents, bonsai_parents, None)
        .await
}

/// Derive the augmented manifest via both paths and assert they agree.
async fn assert_dual_derive_agree(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    aug_parents: &[HgAugmentedManifestId],
    bonsai_parents: (Option<ChangesetId>, Option<ChangesetId>),
) -> Result<HgAugmentedManifestId> {
    assert_dual_derive_agree_at_root(ctx, repo, cs_id, aug_parents, bonsai_parents, None).await
}

/// Like `assert_dual_derive_agree`, but feeds an explicit root `HgManifestId` to
/// both paths instead of the canonical one from the derived `HgChangeset`.
/// `CreateCommitContext` only mints `Generate` roots (canonical == computed), so
/// a `Some(root)` override is the only way to exercise a forced/Supplied root
/// (e.g. a hybrid-mode root) whose id differs from the computed content hash.
async fn assert_dual_derive_agree_at_root(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    aug_parents: &[HgAugmentedManifestId],
    bonsai_parents: (Option<ChangesetId>, Option<ChangesetId>),
    root_override: Option<HgManifestId>,
) -> Result<HgAugmentedManifestId> {
    let acl_root_overlay = derive_acl_overlay(ctx, repo, cs_id).await?;
    let (hg_manifest_id, via_existing_path) = derive_augmented_manifest_via_existing_path(
        ctx,
        repo,
        cs_id,
        aug_parents.to_vec(),
        acl_root_overlay,
        root_override,
    )
    .await?;
    let file_changes = file_changes_from_bonsai(ctx, repo, cs_id).await?;
    let restricted_paths_config = repo.restricted_paths().config_based();

    let via_direct_path = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        ctx,
        repo.repo_blobstore(),
        aug_parents.to_vec(),
        file_changes,
        vec![],
        bonsai_parents,
        &Default::default(),
        Some(hg_manifest_id.into_nodehash()),
        restricted_paths_config,
        acl_root_overlay,
        &mut HashMap::new(),
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

fn direct_derivation_knobs(enabled: bool) -> JustKnobsInMemory {
    JustKnobsInMemory::new(HashMap::from([(
        "scm/mononoke:augmented_manifest_direct_derivation".to_string(),
        KnobVal::Bool(enabled),
    )]))
}

fn restricted_paths_access_logging_knobs(enabled: bool) -> JustKnobsInMemory {
    JustKnobsInMemory::new(HashMap::from([(
        "scm/mononoke:enabled_restricted_paths_access_logging".to_string(),
        KnobVal::Bool(enabled),
    )]))
}

async fn build_repo_with_restricted_path_config(
    fb: FacebookInit,
    restricted_paths: Vec<NonRootMPath>,
) -> Result<Repo> {
    let path_restriction_metadata = restricted_paths
        .into_iter()
        .map(|path| {
            (
                path,
                PathRestrictionMetadata {
                    repo_region_acl: MononokeIdentity::from_legacy_type_data(
                        "REPO_REGION",
                        "test_acl",
                    ),
                    permission_request_group: None,
                    read_only: false,
                },
            )
        })
        .collect();

    let repo = test_repo_factory::TestRepoFactory::new(fb)?
        .with_config_override(move |cfg| {
            cfg.restricted_paths_config = RestrictedPathsConfig {
                path_restriction_metadata,
                ..Default::default()
            };
        })
        .build()
        .await?;
    Ok(repo)
}

async fn create_restricted_path_test_stack(
    ctx: &CoreContext,
    repo: &Repo,
) -> Result<(ChangesetId, ChangesetId)> {
    let parent = CreateCommitContext::new_root(ctx, repo)
        .add_file("restricted/secret.txt", "hidden")
        .add_file("public/normal.txt", "visible")
        .commit()
        .await?;
    let child = CreateCommitContext::new(ctx, repo, vec![parent])
        .add_file("restricted/secret.txt", "hidden v2")
        .add_file("restricted/nested/deep.txt", "deeper")
        .add_file("public/normal.txt", "visible v2")
        .commit()
        .await?;

    Ok((parent, child))
}

async fn derive_augmented_manifest_directly_for_test(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    parents: Vec<HgAugmentedManifestId>,
    bonsai_parents: (Option<ChangesetId>, Option<ChangesetId>),
) -> Result<HgAugmentedManifestId> {
    let expected_root = repo
        .derive_hg_changeset(ctx, cs_id)
        .await?
        .load(ctx, repo.repo_blobstore())
        .await?
        .manifestid()
        .into_nodehash();
    let file_changes = file_changes_from_bonsai(ctx, repo, cs_id).await?;
    let restricted_paths_config = repo.restricted_paths().config_based();

    derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        ctx,
        repo.repo_blobstore(),
        parents,
        file_changes,
        vec![],
        bonsai_parents,
        &Default::default(),
        Some(expected_root),
        restricted_paths_config,
        None,
        &mut HashMap::new(),
    )
    .await
}

async fn hg_augmented_restricted_path_entries(
    ctx: &CoreContext,
    repo: &Repo,
) -> Result<Vec<RestrictedPathManifestIdEntry>> {
    let mut entries: Vec<_> = repo
        .restricted_paths()
        .config_based()
        .manifest_id_store()
        .get_all_entries(ctx)
        .await?
        .into_iter()
        .filter(|entry| entry.manifest_type == ManifestType::HgAugmented)
        .collect();
    entries.sort();
    Ok(entries)
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
    let parent_augmented_manifest_id = derive_parent_aug(&ctx, &repo, parent).await?;
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

/// Helper to derive the parent's augmented manifest using the existing path
/// so we have a parent input for the direct-derivation tests.
async fn derive_parent_aug(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
) -> Result<HgAugmentedManifestId> {
    let acl_root_overlay = derive_acl_overlay(ctx, repo, cs_id).await?;
    Ok(derive_augmented_manifest_via_existing_path(
        ctx,
        repo,
        cs_id,
        vec![],
        acl_root_overlay,
        None,
    )
    .await?
    .1)
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
    let restricted_paths_config = repo.restricted_paths().config_based();
    let direct = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        ctx,
        repo.repo_blobstore(),
        aug_parents.to_vec(),
        file_changes,
        vec![],
        bonsai_parents,
        &Default::default(),
        Some(AS_HASH),
        restricted_paths_config,
        Default::default(),
        &mut HashMap::new(),
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

/// Octopus merge where a Bonsai delete cancels the step-parent's only
/// distinct contribution, making the merged tree equal to p1.
///
/// This is the reuse case not covered by the previous octopus tests: those
/// leave p3's distinct contribution in the merge, so both paths create fresh
/// envelopes and can agree even with a too-tight reuse guard.
#[mononoke::fbinit_test]
async fn test_octopus_merge_bonsai_delete_cancels_step_parent_subdir(
    fb: FacebookInit,
) -> Result<()> {
    // Given: p1 and p2 have identical `dir/` trees, while p3 differs only by
    // adding `dir/extra`. After parent dedup, the directory parents are the
    // shape `[Traced(0, X_dir), Traced(2, Y_dir)]`: more than one parent is
    // present, but only p1 is hg-relevant.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/file", "a")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/file", "a")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("dir/file", "a")
        .add_file("dir/extra", "b")
        .commit()
        .await?;
    let aug_p1 = derive_parent_aug(&ctx, &repo, p1).await?;
    let aug_p2 = derive_parent_aug(&ctx, &repo, p2).await?;
    let aug_p3 = derive_parent_aug(&ctx, &repo, p3).await?;
    assert_eq!(
        aug_p1, aug_p2,
        "Test setup invariant: p1 and p2 must have identical augmented manifest ids",
    );
    assert_ne!(
        aug_p1, aug_p3,
        "Test setup invariant: p3 must differ from p1",
    );

    // When: the merge deletes `dir/extra`, cancelling p3's distinct
    // contribution and making the merged `dir/` content equal to p1.
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .delete_file("dir/extra")
        .commit()
        .await?;

    // Then: direct derivation matches the existing path and reuses p1's root
    // augmented manifest. A guard like `p1.is_some() && p2.is_some()` would
    // skip reuse here and produce a fresh divergent manifest id.
    let derived = assert_dual_derive_agree(
        &ctx,
        &repo,
        merge,
        &[aug_p1, aug_p2, aug_p3],
        (Some(p1), Some(p2)),
    )
    .await?;
    assert_eq!(
        derived, aug_p1,
        "delete-cancelled merge should reuse p1's augmented manifest",
    );

    Ok(())
}

/// Forced-hash root parity: when the canonical Mercurial root id differs from
/// the computed content hash, the direct path must store the envelope at the
/// canonical key so client lookups by `HgChangeset.rootnode` succeed.
///
/// `CreateCommitContext` only produces generated roots where canonical and
/// computed ids match, so the test uses a fabricated supplied id to cover this
/// otherwise-unreachable path.
#[mononoke::fbinit_test]
async fn test_direct_derivation_root_uses_expected_hg_node_id(fb: FacebookInit) -> Result<()> {
    use mercurial_types::HgNodeHash;
    use mononoke_types::sha1_hash::Sha1 as NodeSha1;

    // Given: a root commit and a supplied root id that differs from the natural
    // computed manifest hash.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let cs_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("foo", "bar")
        .add_file("baz/qux", "quux")
        .commit()
        .await?;
    let supplied = HgNodeHash::new(NodeSha1::from_byte_array([0xAB; 20]));
    let file_changes = file_changes_from_bonsai(&ctx, &repo, cs_id).await?;
    let restricted_paths_config = repo.restricted_paths().config_based();

    // When: deriving directly from Bonsai with the supplied root id.
    let aug_id = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        &ctx,
        repo.repo_blobstore(),
        vec![],
        file_changes,
        vec![],
        (None, None),
        &Default::default(),
        Some(supplied),
        restricted_paths_config,
        None,
        &mut HashMap::new(),
    )
    .await?;

    // Then: the envelope is stored and identified by the supplied root id,
    // while `computed_node_id` still records the natural content hash.
    assert_eq!(
        aug_id.into_nodehash(),
        supplied,
        "envelope must be stored at the supplied root key, not the computed key",
    );
    let env = aug_id.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        env.augmented_manifest.hg_node_id, supplied,
        "envelope.hg_node_id must equal the supplied root id",
    );
    let expected_computed_node_id = derive_hg_augmented_manifest::compute_hg_node_id(
        env.augmented_manifest.subentries.clone(),
        &ctx,
        repo.repo_blobstore(),
        &HgParents::new(None, None),
    )
    .await?;
    assert_eq!(
        env.augmented_manifest.computed_node_id, expected_computed_node_id,
        "computed_node_id must reflect the actual content hash",
    );
    assert_ne!(
        env.augmented_manifest.computed_node_id, supplied,
        "computed_node_id must differ from the supplied root id in this fixture",
    );

    Ok(())
}

/// Regression guard: a supplied root id must survive root-level
/// `MergeResult::Reuse`, which bypasses `finalize_envelope`.
#[mononoke::fbinit_test]
async fn test_direct_derivation_root_reuse_uses_expected_hg_node_id(
    fb: FacebookInit,
) -> Result<()> {
    use blobstore::KeyedBlobstore;
    use mercurial_types::HgNodeHash;
    use mercurial_types::blobs::UploadHgNodeHash;
    use mercurial_types::blobs::UploadHgTreeEntry;
    use mercurial_types::blobs::fetch_manifest_envelope;
    use mononoke_types::RepoPath;
    use mononoke_types::sha1_hash::Sha1 as NodeSha1;

    // Given: a no-op merge whose parents have identical root manifests, plus a
    // supplied root id for the merge result.
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
    assert_eq!(
        aug_p1, aug_p2,
        "fixture requires identical parent roots so derive_manifest reuses one",
    );

    let parent_root = repo
        .derive_hg_changeset(&ctx, p1)
        .await?
        .load(&ctx, repo.repo_blobstore())
        .await?
        .manifestid();
    let contents = fetch_manifest_envelope(&ctx, repo.repo_blobstore(), parent_root)
        .await?
        .into_mut()
        .contents;
    let forced_node_id = HgNodeHash::new(NodeSha1::from_byte_array([0xDC; 20]));
    let blobstore_arc: Arc<dyn KeyedBlobstore> = Arc::new(repo.repo_blobstore().clone());
    let (forced_root, upload_fut) = UploadHgTreeEntry {
        upload_node_id: UploadHgNodeHash::Supplied(forced_node_id),
        contents,
        p1: Some(parent_root.into_nodehash()),
        p2: None,
        path: RepoPath::RootPath,
        computed_node_id: None,
    }
    .upload(ctx.clone(), blobstore_arc)?;
    upload_fut.await?;
    assert_eq!(forced_root.into_nodehash(), forced_node_id);
    assert_ne!(forced_root, parent_root);

    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2])
        .commit()
        .await?;
    let acl_root_overlay = derive_acl_overlay(&ctx, &repo, merge).await?;
    let file_changes = file_changes_from_bonsai(&ctx, &repo, merge).await?;
    let restricted_paths_config = repo.restricted_paths().config_based();

    // When: deriving that merge through both the direct and HgManifest-based
    // paths with the supplied root id.
    let via_direct_path = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        &ctx,
        repo.repo_blobstore(),
        vec![aug_p1, aug_p2],
        file_changes,
        vec![],
        (Some(p1), Some(p2)),
        &Default::default(),
        Some(forced_root.into_nodehash()),
        restricted_paths_config,
        acl_root_overlay,
        &mut HashMap::new(),
    )
    .await?;
    let direct_env = via_direct_path.load(&ctx, repo.repo_blobstore()).await?;
    let (_, via_existing_path) = derive_augmented_manifest_via_existing_path(
        &ctx,
        &repo,
        merge,
        vec![aug_p1, aug_p2],
        acl_root_overlay,
        Some(forced_root),
    )
    .await?;

    // Then: the reused root is re-emitted at the supplied id and still matches
    // the existing derivation path.
    assert_eq!(via_direct_path, via_existing_path);
    assert_eq!(via_direct_path.into_nodehash(), forced_node_id);
    assert_eq!(direct_env.augmented_manifest.hg_node_id, forced_node_id);
    assert_ne!(
        direct_env.augmented_manifest.computed_node_id, forced_node_id,
        "computed_node_id should preserve the content-derived hash",
    );
    compare_manifests(&ctx, &repo, forced_root, via_direct_path).await?;

    Ok(())
}

/// When `root_hg_node_id_override` is `None`, the root envelope should use
/// the directly computed node id rather than a canonical/supplied one. This
/// is the future path for Bonsai-native commits that have no
/// `MappedHgChangesetId` mapping.
#[mononoke::fbinit_test]
async fn test_direct_derivation_computed_root_when_none(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;

    let cs_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a.txt", "aaa")
        .commit()
        .await?;

    assert!(
        repo.bonsai_hg_mapping()
            .get_hg_from_bonsai(&ctx, cs_id)
            .await?
            .is_none(),
        "fixture should exercise the no-Hg-mapping root path",
    );

    let file_changes = file_changes_from_bonsai(&ctx, &repo, cs_id).await?;
    let restricted_paths_config = repo.restricted_paths().config_based();

    let aug_id_none = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        &ctx,
        repo.repo_blobstore(),
        vec![],
        file_changes.clone(),
        vec![],
        (None, None),
        &Default::default(),
        None,
        restricted_paths_config,
        None,
        &mut HashMap::new(),
    )
    .await?;

    let env = aug_id_none.load(&ctx, repo.repo_blobstore()).await?;
    assert_eq!(
        env.augmented_manifest.hg_node_id, env.augmented_manifest.computed_node_id,
        "With None root override, hg_node_id should equal computed_node_id",
    );
    assert!(
        repo.bonsai_hg_mapping()
            .get_hg_from_bonsai(&ctx, cs_id)
            .await?
            .is_none(),
        "direct derivation with None must not create an Hg mapping",
    );

    // Verify that passing Some(canonical) gives the same result as the
    // HgChangeset-derived root (ensuring the canonical path still works).
    let canonical_root = repo
        .derive_hg_changeset(&ctx, cs_id)
        .await?
        .load(&ctx, repo.repo_blobstore())
        .await?
        .manifestid()
        .into_nodehash();
    let aug_id_some = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        &ctx,
        repo.repo_blobstore(),
        vec![],
        file_changes,
        vec![],
        (None, None),
        &Default::default(),
        Some(canonical_root),
        restricted_paths_config,
        None,
        &mut HashMap::new(),
    )
    .await?;

    // For a root commit with no forced hash, computed == canonical, so both
    // should produce the same id.
    assert_eq!(
        aug_id_none, aug_id_some,
        "None and Some(canonical) should produce the same root for a \
         non-forced-hash commit",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_direct_derivation_none_rejects_supplied_parent_envelope_reuse(
    fb: FacebookInit,
) -> Result<()> {
    use mercurial_types::HgNodeHash;
    use mononoke_types::sha1_hash::Sha1 as NodeSha1;

    // Given: a no-op merge where p1's augmented root is stored under a supplied
    // root id while p2 has the same content under its content-derived id. This
    // makes `finalize_envelope` consider reusing p1's supplied envelope.
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
    let supplied = HgNodeHash::new(NodeSha1::from_byte_array([0xCD; 20]));
    let restricted_paths_config = repo.restricted_paths().config_based();
    let p1_forced = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        &ctx,
        repo.repo_blobstore(),
        vec![],
        file_changes_from_bonsai(&ctx, &repo, p1).await?,
        vec![],
        (None, None),
        &Default::default(),
        Some(supplied),
        restricted_paths_config,
        None,
        &mut HashMap::new(),
    )
    .await?;
    let p2_canonical = derive_parent_aug(&ctx, &repo, p2).await?;
    assert_ne!(
        p1_forced, p2_canonical,
        "the supplied p1 root must differ from p2's content-derived root",
    );
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2])
        .commit()
        .await?;
    let file_changes = file_changes_from_bonsai(&ctx, &repo, merge).await?;

    // When: deriving the merge with no root override.
    let aug_id = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        &ctx,
        repo.repo_blobstore(),
        vec![p1_forced, p2_canonical],
        file_changes,
        vec![],
        (Some(p1), Some(p2)),
        &Default::default(),
        None,
        restricted_paths_config,
        None,
        &mut HashMap::new(),
    )
    .await?;

    // Then: the no-override path creates a content-derived root instead of
    // reusing p1's supplied envelope.
    let env = aug_id.load(&ctx, repo.repo_blobstore()).await?;
    assert_ne!(aug_id, p1_forced, "must not reuse the supplied parent root");
    assert_eq!(
        env.augmented_manifest.hg_node_id, env.augmented_manifest.computed_node_id,
        "None root override should produce a content-derived root",
    );
    assert_ne!(
        env.augmented_manifest.hg_node_id, supplied,
        "the supplied parent root id must not leak into the None path",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_direct_derivation_none_reemits_supplied_short_circuit_root(
    fb: FacebookInit,
) -> Result<()> {
    use mercurial_types::HgNodeHash;
    use mononoke_types::sha1_hash::Sha1 as NodeSha1;

    // Given: a no-op merge whose parent augmented roots are the same supplied
    // root id, so `derive_manifest` can short-circuit and return that parent
    // root without calling `finalize_envelope`.
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
    let supplied = HgNodeHash::new(NodeSha1::from_byte_array([0xEF; 20]));
    let restricted_paths_config = repo.restricted_paths().config_based();
    let p1_forced = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        &ctx,
        repo.repo_blobstore(),
        vec![],
        file_changes_from_bonsai(&ctx, &repo, p1).await?,
        vec![],
        (None, None),
        &Default::default(),
        Some(supplied),
        restricted_paths_config,
        None,
        &mut HashMap::new(),
    )
    .await?;
    let p2_forced = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        &ctx,
        repo.repo_blobstore(),
        vec![],
        file_changes_from_bonsai(&ctx, &repo, p2).await?,
        vec![],
        (None, None),
        &Default::default(),
        Some(supplied),
        restricted_paths_config,
        None,
        &mut HashMap::new(),
    )
    .await?;
    assert_eq!(p1_forced, p2_forced, "fixture should force root reuse");
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2])
        .commit()
        .await?;

    // When: deriving the merge with no root override.
    let aug_id = derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        &ctx,
        repo.repo_blobstore(),
        vec![p1_forced, p2_forced],
        file_changes_from_bonsai(&ctx, &repo, merge).await?,
        vec![],
        (Some(p1), Some(p2)),
        &Default::default(),
        None,
        restricted_paths_config,
        None,
        &mut HashMap::new(),
    )
    .await?;

    // Then: the root short-circuit is re-emitted as content-derived instead of
    // returning the supplied parent root verbatim.
    let env = aug_id.load(&ctx, repo.repo_blobstore()).await?;
    assert_ne!(
        aug_id, p1_forced,
        "must not return the supplied parent root"
    );
    assert_eq!(
        env.augmented_manifest.hg_node_id, env.augmented_manifest.computed_node_id,
        "None root override should produce a content-derived root",
    );
    assert_ne!(
        env.augmented_manifest.hg_node_id, supplied,
        "the supplied parent root id must not leak through root short-circuit reuse",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_direct_derivation_tracks_restricted_paths_like_existing_path(
    fb: FacebookInit,
) -> Result<()> {
    with_just_knobs_async(
        restricted_paths_access_logging_knobs(true),
        async move {
            // Given: two equivalent repos with a two-commit stack, two configured
            // restriction roots, and an unrestricted public directory.
            let ctx = CoreContext::test_mock(fb);
            let restricted_paths = vec![
                NonRootMPath::new("restricted")?,
                NonRootMPath::new("restricted/nested")?,
            ];
            let existing_repo =
                build_repo_with_restricted_path_config(fb, restricted_paths.clone()).await?;
            let direct_repo = build_repo_with_restricted_path_config(fb, restricted_paths).await?;
            let (existing_parent, existing_child) =
                create_restricted_path_test_stack(&ctx, &existing_repo).await?;
            let (direct_parent, direct_child) =
                create_restricted_path_test_stack(&ctx, &direct_repo).await?;

            // When: deriving one repo through the existing HgManifest-based path and
            // the other through the direct Bonsai-based path.
            let (_, existing_parent_augmented) = derive_augmented_manifest_via_existing_path(
                &ctx,
                &existing_repo,
                existing_parent,
                vec![],
                None,
                None,
            )
            .await?;
            let (_, existing_child_augmented) = derive_augmented_manifest_via_existing_path(
                &ctx,
                &existing_repo,
                existing_child,
                vec![existing_parent_augmented],
                None,
                None,
            )
            .await?;
            let direct_parent_augmented = derive_augmented_manifest_directly_for_test(
                &ctx,
                &direct_repo,
                direct_parent,
                vec![],
                (None, None),
            )
            .await?;
            let direct_child_augmented = derive_augmented_manifest_directly_for_test(
                &ctx,
                &direct_repo,
                direct_child,
                vec![direct_parent_augmented],
                (Some(direct_parent), None),
            )
            .await?;
            let existing_entries =
                hg_augmented_restricted_path_entries(&ctx, &existing_repo).await?;
            let direct_entries = hg_augmented_restricted_path_entries(&ctx, &direct_repo).await?;

            // Then: the direct path writes the same HgAugmented manifest-id entries as
            // the existing path, and only for configured restricted directory roots.
            assert_eq!(direct_parent_augmented, existing_parent_augmented);
            assert_eq!(direct_child_augmented, existing_child_augmented);
            assert_eq!(direct_entries, existing_entries);
            assert_eq!(
                direct_entries.len(),
                3,
                "expected parent restricted/ plus child restricted/ and restricted/nested entries, got: {direct_entries:?}",
            );
            let entry_paths: Vec<_> = direct_entries
                .iter()
                .map(|entry| entry.repo_path())
                .collect::<Result<_>>()?;
            assert!(entry_paths.contains(&RepoPath::dir("restricted")?));
            assert!(entry_paths.contains(&RepoPath::dir("restricted/nested")?));
            assert!(!entry_paths.contains(&RepoPath::dir("public")?));

            Ok(())
        }
        .boxed(),
    )
    .await
}

#[mononoke::fbinit_test]
async fn test_direct_derivation_does_not_track_restricted_paths_when_jk_disabled(
    fb: FacebookInit,
) -> Result<()> {
    with_just_knobs_async(
        restricted_paths_access_logging_knobs(false),
        async move {
            // Given: a repo with configured restriction roots but the access-logging
            // JK disabled.
            let ctx = CoreContext::test_mock(fb);
            let repo = build_repo_with_restricted_path_config(
                fb,
                vec![
                    NonRootMPath::new("restricted")?,
                    NonRootMPath::new("restricted/nested")?,
                ],
            )
            .await?;
            let (parent, child) = create_restricted_path_test_stack(&ctx, &repo).await?;

            // When: deriving the same two-commit stack through the direct path.
            let parent_augmented = derive_augmented_manifest_directly_for_test(
                &ctx,
                &repo,
                parent,
                vec![],
                (None, None),
            )
            .await?;
            derive_augmented_manifest_directly_for_test(
                &ctx,
                &repo,
                child,
                vec![parent_augmented],
                (Some(parent), None),
            )
            .await?;

            // Then: no HgAugmented restricted-path entries are written.
            let entries = hg_augmented_restricted_path_entries(&ctx, &repo).await?;
            assert_eq!(
                entries,
                Vec::<RestrictedPathManifestIdEntry>::new(),
                "restricted-path tracking should be gated entirely by the JK",
            );

            Ok(())
        }
        .boxed(),
    )
    .await
}

/// Recursively compare ACL pointers between two augmented manifests, reading
/// each side from a distinct blobstore so the comparison is genuinely
/// old-vs-new rather than self-vs-self.
///
/// The augmented manifest id IS the Hg node hash (see
/// `HgAugmentedManifestEnvelope::store`), so old and new envelopes share the
/// same blobstore key. To compare their ACL pointer fields we need to load
/// each side from a blobstore that holds ITS write. Callers therefore route
/// each path's writes to its own `MemWritesKeyedBlobstore` overlay and pass
/// the two overlays here; each overlay serves its own write on a `load(...)`
/// of the shared key, and only falls through to the main blobstore for
/// unrelated content (HgManifest blobs, ACL manifest blobs, content blobs).
///
/// Checks the envelope's `acl_manifest_directory_id` AND each directory
/// subentry's `acl_manifest_directory_id` at every level. Also asserts
/// subentry count and per-position kind match, so a missing entry or a
/// directory/leaf swap fails loudly instead of being silently skipped.
async fn compare_augmented_manifests_acl_recursive(
    ctx: &CoreContext,
    old_blobstore: &(impl blobstore::KeyedBlobstore + 'static),
    new_blobstore: &(impl blobstore::KeyedBlobstore + 'static),
    old_id: HgAugmentedManifestId,
    new_id: HgAugmentedManifestId,
    path: mononoke_types::MPath,
) -> Result<()> {
    use mercurial_types::HgAugmentedManifestEntry;

    let old_env = old_id.load(ctx, old_blobstore).await?;
    let new_env = new_id.load(ctx, new_blobstore).await?;

    assert_eq!(
        old_env.augmented_manifest.acl_manifest_directory_id,
        new_env.augmented_manifest.acl_manifest_directory_id,
        "ACL pointer mismatch at path {path:?}",
    );

    let old_entries: Vec<_> = old_env
        .augmented_manifest
        .into_subentries(ctx, old_blobstore)
        .try_collect()
        .await?;
    let new_entries: Vec<_> = new_env
        .augmented_manifest
        .into_subentries(ctx, new_blobstore)
        .try_collect()
        .await?;

    assert_eq!(
        old_entries.len(),
        new_entries.len(),
        "Subentry count mismatch at path {path:?}: old={}, new={}",
        old_entries.len(),
        new_entries.len(),
    );

    for ((old_name, old_entry), (new_name, new_entry)) in old_entries.iter().zip(new_entries.iter())
    {
        assert_eq!(old_name, new_name, "Subentry name mismatch at {path:?}");
        match (old_entry, new_entry) {
            (
                HgAugmentedManifestEntry::DirectoryNode(old_dir),
                HgAugmentedManifestEntry::DirectoryNode(new_dir),
            ) => {
                assert_eq!(
                    old_dir.acl_manifest_directory_id, new_dir.acl_manifest_directory_id,
                    "Child ACL pointer mismatch for {old_name:?} at {path:?}",
                );
                let child_path = path.join(std::iter::once(old_name));
                Box::pin(compare_augmented_manifests_acl_recursive(
                    ctx,
                    old_blobstore,
                    new_blobstore,
                    HgAugmentedManifestId::new(old_dir.treenode),
                    HgAugmentedManifestId::new(new_dir.treenode),
                    child_path,
                ))
                .await?;
            }
            (HgAugmentedManifestEntry::FileNode(_), HgAugmentedManifestEntry::FileNode(_)) => {
                // File leaves carry no ACL pointer; nothing to check here.
            }
            _ => panic!(
                "Entry kind mismatch for {old_name:?} at {path:?}: old={old_entry:?} new={new_entry:?}",
            ),
        }
    }

    Ok(())
}

/// Derive `cs_id` via the new direct path into `overlay`. Pulls
/// `root_hg_node_id_override` from the already-derived HgChangeset and
/// `acl_root_overlay` from RootAclManifestId. Used by the ACL parity test
/// alongside `derive_existing_into_overlay` to keep each path's envelope
/// writes in its own blobstore.
async fn derive_into_overlay<B>(
    ctx: &CoreContext,
    repo: &Repo,
    overlay: &B,
    cs_id: ChangesetId,
    aug_parents: Vec<HgAugmentedManifestId>,
    bonsai_parents: (Option<ChangesetId>, Option<ChangesetId>),
) -> Result<HgAugmentedManifestId>
where
    B: blobstore::KeyedBlobstore + Clone + 'static,
{
    derive_into_overlay_with_subtrees(
        ctx,
        repo,
        overlay,
        cs_id,
        aug_parents,
        bonsai_parents,
        &HashMap::new(),
    )
    .await
}

async fn derive_into_overlay_with_subtrees<B>(
    ctx: &CoreContext,
    repo: &Repo,
    overlay: &B,
    cs_id: ChangesetId,
    aug_parents: Vec<HgAugmentedManifestId>,
    bonsai_parents: (Option<ChangesetId>, Option<ChangesetId>),
    subtree_source_augs: &HashMap<ChangesetId, HgAugmentedManifestId>,
) -> Result<HgAugmentedManifestId>
where
    B: blobstore::KeyedBlobstore + Clone + 'static,
{
    let acl_root_overlay = derive_acl_overlay(ctx, repo, cs_id).await?;
    let bonsai: mononoke_types::BonsaiChangeset = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let expected_root = repo
        .derive_hg_changeset(ctx, cs_id)
        .await?
        .load(ctx, repo.repo_blobstore())
        .await?
        .manifestid()
        .into_nodehash();
    let file_changes = file_changes_from_bonsai(ctx, repo, cs_id).await?;
    let subtree_replacements = derive_hg_augmented_manifest::build_augmented_subtree_replacements(
        ctx,
        overlay,
        &bonsai,
        subtree_source_augs,
    )
    .await?;
    derive_hg_augmented_manifest::derive_augmented_manifest_from_bonsai(
        ctx,
        overlay,
        aug_parents,
        file_changes,
        subtree_replacements,
        bonsai_parents,
        &Default::default(),
        Some(expected_root),
        repo.restricted_paths().config_based(),
        acl_root_overlay,
        &mut HashMap::new(),
    )
    .await
}

/// Derive `cs_id` via the existing HgManifest-based path into `overlay`.
/// Mirror of `derive_into_overlay` for the existing path. Used by the ACL
/// parity test to keep the existing path's writes isolated from the direct
/// path's so the comparison is genuinely existing-vs-direct (and doesn't
/// rely on `PutBehaviour::IfAbsent` in the shared main blobstore, which
/// would silently drop one side's writes on key collision).
async fn derive_existing_into_overlay<B>(
    ctx: &CoreContext,
    repo: &Repo,
    overlay: &B,
    cs_id: ChangesetId,
    aug_parents: Vec<HgAugmentedManifestId>,
) -> Result<HgAugmentedManifestId>
where
    B: blobstore::KeyedBlobstore + Clone + 'static,
{
    let acl_root_overlay = derive_acl_overlay(ctx, repo, cs_id).await?;
    let hg_manifest_id = repo
        .derive_hg_changeset(ctx, cs_id)
        .await?
        .load(ctx, repo.repo_blobstore())
        .await?
        .manifestid();
    derive_hg_augmented_manifest::derive_from_hg_manifest_and_parents(
        ctx,
        overlay,
        hg_manifest_id,
        aug_parents,
        &Default::default(),
        repo.restricted_paths().config_based(),
        acl_root_overlay,
    )
    .await
}

async fn derive_acl_parity_pair<OldStore, NewStore>(
    ctx: &CoreContext,
    repo: &Repo,
    old_blobstore: &OldStore,
    new_blobstore: &NewStore,
    cs_id: ChangesetId,
    old_parents: Vec<HgAugmentedManifestId>,
    new_parents: Vec<HgAugmentedManifestId>,
    bonsai_parents: (Option<ChangesetId>, Option<ChangesetId>),
) -> Result<(HgAugmentedManifestId, HgAugmentedManifestId)>
where
    OldStore: blobstore::KeyedBlobstore + Clone + 'static,
    NewStore: blobstore::KeyedBlobstore + Clone + 'static,
{
    let old_id = derive_existing_into_overlay(ctx, repo, old_blobstore, cs_id, old_parents).await?;
    let new_id =
        derive_into_overlay(ctx, repo, new_blobstore, cs_id, new_parents, bonsai_parents).await?;
    Ok((old_id, new_id))
}

async fn assert_augmented_manifest_acl_parity<OldStore, NewStore>(
    ctx: &CoreContext,
    old_blobstore: &OldStore,
    new_blobstore: &NewStore,
    old_id: HgAugmentedManifestId,
    new_id: HgAugmentedManifestId,
    case_name: &str,
) -> Result<()>
where
    OldStore: blobstore::KeyedBlobstore + 'static,
    NewStore: blobstore::KeyedBlobstore + 'static,
{
    assert_eq!(new_id, old_id, "{case_name}: augmented manifest id parity");
    compare_augmented_manifests_acl_recursive(
        ctx,
        old_blobstore,
        new_blobstore,
        old_id,
        new_id,
        mononoke_types::MPath::ROOT,
    )
    .await
}

/// Root commits with nested restriction roots should match the existing path's
/// ACL pointers at every directory envelope and directory subentry.
#[mononoke::fbinit_test]
async fn test_direct_derivation_matches_existing_acl_pointers_for_root_commit(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a root commit with a top-level restriction root and a nested one.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let old_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let new_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "restricted/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n",
        )
        .add_file("restricted/code/secret.rs", "fn secret() {}")
        .add_file(
            "restricted/code/inner/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project2\"\n",
        )
        .add_file("restricted/code/inner/deep.rs", "fn deep() {}")
        .add_file("public/readme.md", "hello")
        .commit()
        .await?;

    // When: deriving that root commit through both augmented-manifest paths.
    let (old_root, new_root) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        root,
        vec![],
        vec![],
        (None, None),
    )
    .await?;

    // Then: the direct path matches the existing path, including recursive ACL pointers.
    assert_augmented_manifest_acl_parity(
        &ctx,
        &*old_blobstore,
        &*new_blobstore,
        old_root,
        new_root,
        "root commit with nested restrictions",
    )
    .await
}

/// Single-parent file additions under an existing restricted subtree should
/// preserve the existing path's ACL pointers.
#[mononoke::fbinit_test]
async fn test_direct_derivation_matches_existing_acl_pointers_for_restricted_file_add(
    fb: FacebookInit,
) -> Result<()> {
    // Given: an already-derived restricted parent and a child that only adds a file inside it.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let old_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let new_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "restricted/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n",
        )
        .add_file("restricted/code/secret.rs", "fn secret() {}")
        .commit()
        .await?;
    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("restricted/code/more.rs", "fn more() {}")
        .commit()
        .await?;
    let (old_root, new_root) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        root,
        vec![],
        vec![],
        (None, None),
    )
    .await?;

    // When: deriving the child commit through both augmented-manifest paths.
    let (old_child, new_child) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        child,
        vec![old_root],
        vec![new_root],
        (Some(root), None),
    )
    .await?;

    // Then: the direct path matches the existing path, including recursive ACL pointers.
    assert_augmented_manifest_acl_parity(
        &ctx,
        &*old_blobstore,
        &*new_blobstore,
        old_child,
        new_child,
        "single-parent file add under restricted subtree",
    )
    .await
}

/// Deleting a `.slacl` file should make the direct path drop ACL pointers in
/// the same places as the existing path.
#[mononoke::fbinit_test]
async fn test_direct_derivation_matches_existing_acl_pointers_for_slacl_delete(
    fb: FacebookInit,
) -> Result<()> {
    // Given: an already-derived chain where the target commit deletes a nested `.slacl`.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let old_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let new_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "restricted/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n",
        )
        .add_file(
            "restricted/code/inner/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project2\"\n",
        )
        .add_file("restricted/code/inner/deep.rs", "fn deep() {}")
        .commit()
        .await?;
    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("restricted/code/more.rs", "fn more() {}")
        .commit()
        .await?;
    let target = CreateCommitContext::new(&ctx, &repo, vec![child])
        .delete_file("restricted/code/inner/.slacl")
        .commit()
        .await?;
    let (old_root, new_root) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        root,
        vec![],
        vec![],
        (None, None),
    )
    .await?;
    let (old_child, new_child) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        child,
        vec![old_root],
        vec![new_root],
        (Some(root), None),
    )
    .await?;

    // When: deriving the `.slacl` deletion commit through both augmented-manifest paths.
    let (old_target, new_target) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        target,
        vec![old_child],
        vec![new_child],
        (Some(child), None),
    )
    .await?;

    // Then: the direct path matches the existing path, including recursive ACL pointers.
    assert_augmented_manifest_acl_parity(
        &ctx,
        &*old_blobstore,
        &*new_blobstore,
        old_target,
        new_target,
        "nested .slacl deletion",
    )
    .await
}

/// Reused sibling subtrees should keep their parent-stored ACL pointer while a
/// different sibling's `.slacl` changes.
#[mononoke::fbinit_test]
async fn test_direct_derivation_preserves_acl_pointer_for_reused_sibling_subtree(
    fb: FacebookInit,
) -> Result<()> {
    // Given: two sibling restriction roots and a child that deletes only one sibling's `.slacl`.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let old_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let new_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "restricted/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=top\"\n",
        )
        .add_file(
            "restricted/keep/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=keep\"\n",
        )
        .add_file("restricted/keep/file.rs", "fn keep() {}")
        .add_file(
            "restricted/other/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=other\"\n",
        )
        .add_file("restricted/other/file.rs", "fn other() {}")
        .commit()
        .await?;
    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .delete_file("restricted/other/.slacl")
        .commit()
        .await?;
    let (old_root, new_root) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        root,
        vec![],
        vec![],
        (None, None),
    )
    .await?;

    // When: deriving the sibling `.slacl` deletion commit through both paths.
    let (old_child, new_child) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        child,
        vec![old_root],
        vec![new_root],
        (Some(root), None),
    )
    .await?;

    // Then: the reused sibling subtree's ACL pointer still matches the existing path.
    assert_augmented_manifest_acl_parity(
        &ctx,
        &*old_blobstore,
        &*new_blobstore,
        old_child,
        new_child,
        "reused sibling subtree across .slacl deletion",
    )
    .await
}

/// Clean two-parent merges with ACL roots on both branches should match the
/// existing path, including reused subtrees from the common base.
#[mononoke::fbinit_test]
async fn test_direct_derivation_matches_existing_acl_pointers_for_clean_acl_merge(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a merge whose two branches add independent restriction roots.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let old_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let new_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "shared/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=shared\"\n",
        )
        .add_file("shared/file.rs", "fn shared() {}")
        .commit()
        .await?;
    let branch_a = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file(
            "branch_a/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project_a\"\n",
        )
        .add_file("branch_a/a.rs", "fn a() {}")
        .commit()
        .await?;
    let branch_b = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file(
            "branch_b/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project_b\"\n",
        )
        .add_file("branch_b/b.rs", "fn b() {}")
        .commit()
        .await?;
    let merge = CreateCommitContext::new(&ctx, &repo, vec![branch_a, branch_b])
        .commit()
        .await?;
    let (old_base, new_base) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        base,
        vec![],
        vec![],
        (None, None),
    )
    .await?;
    let (old_a, new_a) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        branch_a,
        vec![old_base],
        vec![new_base],
        (Some(base), None),
    )
    .await?;
    let (old_b, new_b) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        branch_b,
        vec![old_base],
        vec![new_base],
        (Some(base), None),
    )
    .await?;

    // When: deriving the merge commit through both augmented-manifest paths.
    let (old_merge, new_merge) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        merge,
        vec![old_a, old_b],
        vec![new_a, new_b],
        (Some(branch_a), Some(branch_b)),
    )
    .await?;

    // Then: the direct path matches the existing path, including recursive ACL pointers.
    assert_augmented_manifest_acl_parity(
        &ctx,
        &*old_blobstore,
        &*new_blobstore,
        old_merge,
        new_merge,
        "clean ACL merge",
    )
    .await
}

/// Merges can rebuild restricted directories that are not reachable from
/// `file_changes`; this locks the full ACL-walk fallback for merges.
#[mononoke::fbinit_test]
async fn test_direct_derivation_matches_existing_acl_pointers_for_merge_divergent_dir(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a merge where parent trees diverge under `proj/`, but the merge's
    // changed-file frontier only includes an unrelated root file.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let old_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let new_blobstore = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "proj/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=proj\"\n",
        )
        .add_file("proj/common.rs", "fn common() {}")
        .commit()
        .await?;
    let branch_a = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("proj/extra.rs", "fn extra() {}")
        .commit()
        .await?;
    let branch_b = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("unrelated.rs", "fn unrelated() {}")
        .commit()
        .await?;
    let merge = CreateCommitContext::new(&ctx, &repo, vec![branch_a, branch_b])
        .commit()
        .await?;
    let (old_base, new_base) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        base,
        vec![],
        vec![],
        (None, None),
    )
    .await?;
    let (old_a, new_a) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        branch_a,
        vec![old_base],
        vec![new_base],
        (Some(base), None),
    )
    .await?;
    let (old_b, new_b) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        branch_b,
        vec![old_base],
        vec![new_base],
        (Some(base), None),
    )
    .await?;

    // When: deriving the merge commit through both augmented-manifest paths.
    let (old_merge, new_merge) = derive_acl_parity_pair(
        &ctx,
        &repo,
        &*old_blobstore,
        &*new_blobstore,
        merge,
        vec![old_a, old_b],
        vec![new_a, new_b],
        (Some(branch_a), Some(branch_b)),
    )
    .await?;

    // Then: the direct path's merge fallback preserves `proj/`'s ACL pointer.
    assert_augmented_manifest_acl_parity(
        &ctx,
        &*old_blobstore,
        &*new_blobstore,
        old_merge,
        new_merge,
        "merge rebuild of parent-divergent restricted dir",
    )
    .await
}

/// `targeted_acl_overlay_map` should load the touched ACL branch and immediate
/// sibling pointers, but not descend into untouched sibling subtrees.
#[mononoke::fbinit_test]
async fn test_targeted_acl_overlay_map_is_scoped_to_changed_frontier(
    fb: FacebookInit,
) -> Result<()> {
    // Given: two sibling ACL subtrees and a target frontier under only one sibling.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let cs_id = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "a/inner/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=a\"\n",
        )
        .add_file("a/inner/x.rs", "fn x() {}")
        .add_file(
            "b/inner/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=b\"\n",
        )
        .add_file("b/inner/y.rs", "fn y() {}")
        .commit()
        .await?;
    let root_acl_id = derive_acl_overlay(&ctx, &repo, cs_id)
        .await?
        .expect("repo has .slacl files, so the overlay root must be Some");
    let dir = |s: &str| -> Result<MPath> { Ok(MPath::from(NonRootMPath::new(s)?)) };
    let target_dirs: HashSet<MPath> = [MPath::ROOT, dir("a")?, dir("a/inner")?]
        .into_iter()
        .collect();

    // When: loading the targeted ACL overlay map for that frontier.
    let map = derive_hg_augmented_manifest::targeted_acl_overlay_map(
        &ctx,
        repo.repo_blobstore(),
        root_acl_id,
        &target_dirs,
    )
    .await?;

    // Then: the touched branch and immediate sibling pointer are present, but
    // the untouched sibling subtree is not loaded.
    assert!(map.contains_key(&MPath::ROOT), "root pointer recorded");
    assert!(map.contains_key(&dir("a")?), "a/ descended");
    assert!(
        map.contains_key(&dir("a/inner")?),
        "a/inner restriction root recorded"
    );
    assert!(
        map.contains_key(&dir("b")?),
        "b/ recorded as an immediate child of the rebuilt root"
    );
    assert!(
        !map.contains_key(&dir("b/inner")?),
        "b/inner must NOT be loaded: it is outside the target frontier"
    );

    Ok(())
}

/// Derive both paths over a sequence of changesets in topological order and
/// compare the results. Each path writes into its own `MemWritesKeyedBlobstore`
/// overlay for proper isolation (augmented manifest IDs are Hg node hashes, so
/// both paths write the same blobstore key — without separate overlays,
/// `IfAbsent` semantics would mask divergent envelopes).
async fn assert_direct_derivation_parity(
    ctx: &CoreContext,
    repo: &Repo,
    topo_csids: &[ChangesetId],
) -> Result<()> {
    let old_bs = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));
    let new_bs = Arc::new(MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone()));

    let mut old_augs: HashMap<ChangesetId, HgAugmentedManifestId> = HashMap::new();
    let mut new_augs: HashMap<ChangesetId, HgAugmentedManifestId> = HashMap::new();

    for cs_id in topo_csids {
        let bonsai: mononoke_types::BonsaiChangeset =
            cs_id.load(ctx, repo.repo_blobstore()).await?;
        let old_parents: Vec<_> = bonsai
            .parents()
            .map(|p| {
                *old_augs
                    .get(&p)
                    .unwrap_or_else(|| panic!("Parent {p} not in topo_csids or out of order"))
            })
            .collect();
        let new_parents: Vec<_> = bonsai
            .parents()
            .map(|p| {
                *new_augs
                    .get(&p)
                    .unwrap_or_else(|| panic!("Parent {p} not in topo_csids or out of order"))
            })
            .collect();
        let mut parents = bonsai.parents();
        let bonsai_parents = (parents.next(), parents.next());

        let old_id = derive_existing_into_overlay(ctx, repo, &*old_bs, *cs_id, old_parents).await?;
        let new_id = derive_into_overlay_with_subtrees(
            ctx,
            repo,
            &*new_bs,
            *cs_id,
            new_parents,
            bonsai_parents,
            &new_augs,
        )
        .await?;

        assert_eq!(old_id, new_id, "Root ID mismatch for {cs_id}");

        let old_env = old_id.load(ctx, &*old_bs).await?;
        let new_env = new_id.load(ctx, &*new_bs).await?;
        assert_eq!(
            old_env.augmented_manifest_id, new_env.augmented_manifest_id,
            "Blake3 digest mismatch for {cs_id}"
        );

        compare_augmented_manifests_acl_recursive(
            ctx,
            &*old_bs,
            &*new_bs,
            old_id,
            new_id,
            mononoke_types::MPath::ROOT,
        )
        .await
        .with_context(|| format!("ACL parity failure at {cs_id}"))?;

        old_augs.insert(*cs_id, old_id);
        new_augs.insert(*cs_id, new_id);
    }

    Ok(())
}

/// Octopus copy-from from the two Mercurial-visible parents should preserve
/// Bonsai parent order when matching copy sources to augmented-manifest parents.
#[mononoke::fbinit_test]
async fn test_direct_derivation_octopus_copy_from_respects_hg_parent_order(
    fb: FacebookInit,
) -> Result<()> {
    // Given: three independent parents ordered reverse by ChangesetId, so a
    // bug that sorted parents by id would swap p1/p2 copy-from source slots.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let root_a = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src_a", "content_a")
        .commit()
        .await?;
    let root_b = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src_b", "content_b")
        .commit()
        .await?;
    let root_c = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src_c", "content_c")
        .commit()
        .await?;
    let mut roots = [
        (root_a, "src_a", "content_a"),
        (root_b, "src_b", "content_b"),
        (root_c, "src_c", "content_c"),
    ];
    roots.sort_by_key(|(cs, _, _)| *cs);
    roots.reverse();
    let [(p1, s1, c1), (p2, s2, c2), (p3, _, _)] = roots;
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .add_file_with_copy_info("dst_from_p1", c1, (p1, s1))
        .add_file_with_copy_info("dst_from_p2", c2, (p2, s2))
        .commit()
        .await?;

    // When: deriving the octopus merge through both augmented-manifest paths.
    let result = assert_direct_derivation_parity(&ctx, &repo, &[p1, p2, p3, merge]).await;

    // Then: direct derivation matches the existing path's p1/p2 copy-from parentage.
    result
}

/// Octopus copy-from from a step-parent should be ignored, matching Mercurial's
/// two-parent filenode model and the existing HgManifest-based path.
#[mononoke::fbinit_test]
async fn test_direct_derivation_octopus_copy_from_ignores_step_parent(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a three-parent merge whose only copied file names p3 as its source.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let p1 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src_p1", "content_p1")
        .commit()
        .await?;
    let p2 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src_p2", "content_p2")
        .commit()
        .await?;
    let p3 = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src_p3", "content_p3")
        .commit()
        .await?;
    let merge = CreateCommitContext::new(&ctx, &repo, vec![p1, p2, p3])
        .add_file_with_copy_info("dst_from_p3", "content_p3", (p3, "src_p3"))
        .commit()
        .await?;

    // When: deriving the octopus merge through both augmented-manifest paths.
    let result = assert_direct_derivation_parity(&ctx, &repo, &[p1, p2, p3, merge]).await;

    // Then: direct derivation matches the existing path, which drops p3 copy-from metadata.
    result
}

// Subtree-copy parity tests.

/// Create and save a bonsai with `subtree_changes`.
async fn save_bonsai_with_subtree_changes(
    ctx: &CoreContext,
    repo: &Repo,
    parents: Vec<ChangesetId>,
    subtree_changes: Vec<(MPath, SubtreeChange)>,
    file_changes: Vec<(&str, Option<&str>)>,
) -> Result<ChangesetId> {
    use changesets_creation::save_changesets;

    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (
                "scm/mononoke:enable_subtree_changes".to_string(),
                KnobVal::Bool(true),
            ),
            (
                "scm/mononoke:enable_manifest_altering_subtree_changes".to_string(),
                KnobVal::Bool(true),
            ),
        ])),
        async move {
            let mut builder =
                CreateCommitContext::new(ctx, repo, parents).set_message("subtree change commit");
            for (path, content) in file_changes {
                builder = match content {
                    Some(content) => builder.add_file(path, content),
                    None => builder.delete_file(path),
                };
            }

            let mut bonsai = builder.create_commit_object().await?;
            bonsai.subtree_changes = subtree_changes.into_iter().collect();
            let bonsai = bonsai.freeze()?;
            let cs_id = bonsai.get_changeset_id();
            save_changesets(ctx, repo, vec![bonsai]).await?;
            Ok(cs_id)
        }
        .boxed(),
    )
    .await
}

fn subtree_copy(
    to_path: &str,
    from_path: &str,
    from_cs_id: ChangesetId,
) -> Result<(MPath, SubtreeChange)> {
    Ok((
        MPath::new(to_path)?,
        SubtreeChange::copy(MPath::new(from_path)?, from_cs_id),
    ))
}

/// Exact directory subtree copy.
#[mononoke::fbinit_test]
async fn test_direct_derivation_subtree_exact_directory_copy_matches_existing_path(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a source commit with a directory subtree, including ACL metadata.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let source = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "src/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=outer\"\n",
        )
        .add_file("src/code.rs", "fn outer() {}")
        .add_file(
            "src/inner/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=inner\"\n",
        )
        .add_file("src/inner/deep.rs", "fn inner() {}")
        .commit()
        .await?;

    // When: a child copies that directory to a new destination path.
    let child = save_bonsai_with_subtree_changes(
        &ctx,
        &repo,
        vec![source],
        vec![subtree_copy("dst", "src", source)?],
        vec![],
    )
    .await?;
    let result = assert_direct_derivation_parity(&ctx, &repo, &[source, child]).await;

    // Then: direct derivation matches the existing HgManifest-based path.
    result
}

/// Exact file subtree copy.
#[mononoke::fbinit_test]
async fn test_direct_derivation_subtree_exact_file_copy_matches_existing_path(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a source commit with one file that will be copied as a subtree change.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let source = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/file.rs", "fn source() {}")
        .commit()
        .await?;

    // When: a child copies that file to a new destination path.
    let child = save_bonsai_with_subtree_changes(
        &ctx,
        &repo,
        vec![source],
        vec![subtree_copy("copied.rs", "src/file.rs", source)?],
        vec![],
    )
    .await?;
    let result = assert_direct_derivation_parity(&ctx, &repo, &[source, child]).await;

    // Then: direct derivation matches the existing HgManifest-based path.
    result
}

/// Multiple subtree copies in one commit.
#[mononoke::fbinit_test]
async fn test_direct_derivation_multiple_subtree_copies_match_existing_path(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a source commit with two independent source directories.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let source = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src_a/a.txt", "a")
        .add_file("src_b/b.txt", "b")
        .commit()
        .await?;

    // When: a child performs two subtree copies, requiring two synthetic parents.
    let child = save_bonsai_with_subtree_changes(
        &ctx,
        &repo,
        vec![source],
        vec![
            subtree_copy("dst_a", "src_a", source)?,
            subtree_copy("dst_b", "src_b", source)?,
        ],
        vec![],
    )
    .await?;
    let result = assert_direct_derivation_parity(&ctx, &repo, &[source, child]).await;

    // Then: direct derivation matches the existing HgManifest-based path.
    result
}

/// Subtree copy from a changeset that is not a parent of the copying commit.
#[mononoke::fbinit_test]
async fn test_direct_derivation_subtree_copy_from_non_parent_source_matches_existing_path(
    fb: FacebookInit,
) -> Result<()> {
    // Given: an unrelated source commit and a separate base commit for the child.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let source = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/lib.rs", "fn from_source() {}")
        .add_file("src/data.txt", "source data")
        .commit()
        .await?;
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base/readme.txt", "base")
        .commit()
        .await?;

    // When: the child copies a subtree from `source`, even though its real parent is `base`.
    let child = save_bonsai_with_subtree_changes(
        &ctx,
        &repo,
        vec![base],
        vec![subtree_copy("dst", "src", source)?],
        vec![],
    )
    .await?;
    let result = assert_direct_derivation_parity(&ctx, &repo, &[source, base, child]).await;

    // Then: direct derivation resolves the non-parent source and matches the existing path.
    result
}

/// Directory copy with normal file changes layered on the copied baseline.
#[mononoke::fbinit_test]
async fn test_direct_derivation_subtree_copy_with_modify_and_delete_matches_existing_path(
    fb: FacebookInit,
) -> Result<()> {
    // Given: a source directory with files that can be modified or deleted after copy.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/a.txt", "old a")
        .add_file("src/b.txt", "old b")
        .add_file("src/c.txt", "old c")
        .commit()
        .await?;

    // When: a child copies the directory and overlays a modify plus a delete.
    let child = save_bonsai_with_subtree_changes(
        &ctx,
        &repo,
        vec![root],
        vec![subtree_copy("dst", "src", root)?],
        vec![("dst/a.txt", Some("new a")), ("dst/b.txt", None)],
    )
    .await?;
    let result = assert_direct_derivation_parity(&ctx, &repo, &[root, child]).await;

    // Then: direct derivation matches the existing HgManifest-based path.
    result
}

/// Exact subtree copy into restricted destination paths.
#[mononoke::fbinit_test]
async fn test_direct_derivation_subtree_copy_tracks_restricted_destination(
    fb: FacebookInit,
) -> Result<()> {
    with_just_knobs_async(
        restricted_paths_access_logging_knobs(true),
        async move {
            // Given: restricted-path logging is enabled for the copied destination and its child.
            let ctx = CoreContext::test_mock(fb);
            let repo = build_repo_with_restricted_path_config(
                fb,
                vec![NonRootMPath::new("dst")?, NonRootMPath::new("dst/inner")?],
            )
            .await?;
            let source = CreateCommitContext::new_root(&ctx, &repo)
                .add_file("src/secret.txt", "secret")
                .add_file("src/inner/deep.txt", "deep secret")
                .commit()
                .await?;
            let child = save_bonsai_with_subtree_changes(
                &ctx,
                &repo,
                vec![source],
                vec![subtree_copy("dst", "src", source)?],
                vec![],
            )
            .await?;

            // When: deriving the exact subtree copy through the direct path.
            let overlay = MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone());
            let source_aug =
                derive_into_overlay(&ctx, &repo, &overlay, source, vec![], (None, None)).await?;
            let subtree_source_augs = HashMap::from([(source, source_aug)]);
            derive_into_overlay_with_subtrees(
                &ctx,
                &repo,
                &overlay,
                child,
                vec![source_aug],
                (Some(source), None),
                &subtree_source_augs,
            )
            .await?;

            // Then: both destination restriction roots are tracked for HgAugmented manifests.
            let entries = hg_augmented_restricted_path_entries(&ctx, &repo).await?;
            for expected_path in [RepoPath::dir("dst")?, RepoPath::dir("dst/inner")?] {
                assert!(
                    entries
                        .iter()
                        .any(|entry| entry.repo_path().is_ok_and(|path| path == expected_path)),
                    "expected an HgAugmented restricted-path entry for {expected_path:?}, got {entries:?}",
                );
            }

            Ok(())
        }
        .boxed(),
    )
    .await
}

/// Reproduces restricted-paths tracking gap `new-restricted-paths-tracking-1`.
///
/// An exact whole-directory subtree copy tracks nested restriction roots via
/// `track_restricted_paths_for_existing_directory`. When the copy destination
/// also carries overlapping file changes, `dst/` is rebuilt and an unchanged
/// restriction root below it is spliced in as a reused partial map
/// (`LookupSubtree`). The direct path must still track the destination path,
/// matching the legacy HgManifest path that walks the rebuilt `dst/` as `New`.
#[mononoke::fbinit_test]
async fn test_direct_derivation_subtree_copy_rebuilt_dest_tracks_nested_restricted(
    fb: FacebookInit,
) -> Result<()> {
    with_just_knobs_async(
        restricted_paths_access_logging_knobs(true),
        async move {
            // Given: only the nested directory `dst/inner` is a restriction root.
            let ctx = CoreContext::test_mock(fb);
            let repo = build_repo_with_restricted_path_config(
                fb,
                vec![NonRootMPath::new("dst/inner")?],
            )
            .await?;
            let source = CreateCommitContext::new_root(&ctx, &repo)
                .add_file("src/a.txt", "old a")
                .add_file("src/b.txt", "old b")
                .add_file("src/inner/deep.txt", "deep secret")
                .commit()
                .await?;

            // When: a child copies `src` to `dst` and overlays file changes that rebuild `dst/`.
            let child = save_bonsai_with_subtree_changes(
                &ctx,
                &repo,
                vec![source],
                vec![subtree_copy("dst", "src", source)?],
                vec![("dst/a.txt", Some("new a")), ("dst/b.txt", None)],
            )
            .await?;
            let overlay = MemWritesKeyedBlobstore::new(repo.repo_blobstore().clone());
            let source_aug =
                derive_into_overlay(&ctx, &repo, &overlay, source, vec![], (None, None)).await?;
            let subtree_source_augs = HashMap::from([(source, source_aug)]);
            derive_into_overlay_with_subtrees(
                &ctx,
                &repo,
                &overlay,
                child,
                vec![source_aug],
                (Some(source), None),
                &subtree_source_augs,
            )
            .await?;

            // Then: the reused copied partial map still records the destination restriction root.
            let entries = hg_augmented_restricted_path_entries(&ctx, &repo).await?;
            let expected_path = RepoPath::dir("dst/inner")?;
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.repo_path().is_ok_and(|path| path == expected_path)),
                "expected an HgAugmented restricted-path entry for {expected_path:?}, got {entries:?}",
            );

            Ok(())
        }
        .boxed(),
    )
    .await
}

/// Direct manager derivation with a subtree copy whose source was already
/// persisted must resolve that source from stored `RootHgAugmentedManifestId`
/// data when the direct-derivation JustKnob is enabled.
#[mononoke::fbinit_test]
async fn test_direct_derivation_subtree_persisted_source_with_jk(fb: FacebookInit) -> Result<()> {
    // Given: a subtree-copy source whose augmented root is already persisted,
    // and a later child batch that does not include that source in its local `res` map.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let source = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("lib/mod.rs", "pub mod foo;")
        .add_file("lib/foo.rs", "fn foo() {}")
        .commit()
        .await?;
    let parent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("readme.md", "hello")
        .commit()
        .await?;
    let child = save_bonsai_with_subtree_changes(
        &ctx,
        &repo,
        vec![parent],
        vec![subtree_copy("vendor/lib", "lib", source)?],
        vec![],
    )
    .await?;
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, vec![source, parent, child], None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![source, parent, child], None)
        .await?;
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, vec![source, parent], None)
        .await?;

    // When: deriving only the copying child with direct derivation enabled.
    with_just_knobs_async(
        direct_derivation_knobs(true),
        async {
            manager
                .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, vec![child], None)
                .await
        }
        .boxed(),
    )
    .await?;

    // Then: the direct manager result matches the Hg manifest for the subtree-copy commit.
    let aug_id = manager
        .fetch_derived::<RootHgAugmentedManifestId>(&ctx, child, None)
        .await?
        .expect("derived above")
        .hg_augmented_manifest_id();
    let hg_manifest_id = repo
        .derive_hg_changeset(&ctx, child)
        .await?
        .load(&ctx, repo.repo_blobstore())
        .await?
        .manifestid();
    compare_manifests(&ctx, &repo, hg_manifest_id, aug_id).await
}

/// Direct manager batch derivation must use the batch-local augmented-root map
/// for subtree-copy sources that are derived earlier in the same batch but have
/// not yet been persisted by the manager.
#[mononoke::fbinit_test]
async fn test_direct_derivation_subtree_batch_local_source_with_jk(fb: FacebookInit) -> Result<()> {
    // Given: a single direct-derivation batch containing both a subtree-copy
    // source and the later changeset that copies from it.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let source = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("lib/mod.rs", "pub mod foo;")
        .add_file("lib/foo.rs", "fn foo() {}")
        .commit()
        .await?;
    let parent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("readme.md", "hello")
        .commit()
        .await?;
    let child = save_bonsai_with_subtree_changes(
        &ctx,
        &repo,
        vec![parent],
        vec![subtree_copy("vendor/lib", "lib", source)?],
        vec![],
    )
    .await?;
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, vec![source, parent, child], None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![source, parent, child], None)
        .await?;
    assert!(
        manager
            .fetch_derived::<RootHgAugmentedManifestId>(&ctx, source, None)
            .await?
            .is_none(),
        "source augmented root must not be persisted before the direct batch",
    );

    // When: deriving the source, parent, and subtree-copy child in one JK-on batch.
    with_just_knobs_async(
        direct_derivation_knobs(true),
        async {
            manager
                .derive_exactly_batch::<RootHgAugmentedManifestId>(
                    &ctx,
                    vec![source, parent, child],
                    None,
                )
                .await
        }
        .boxed(),
    )
    .await?;

    // Then: the child resolves its source from the batch-local map and matches the Hg manifest.
    let aug_id = manager
        .fetch_derived::<RootHgAugmentedManifestId>(&ctx, child, None)
        .await?
        .expect("derived above")
        .hg_augmented_manifest_id();
    let hg_manifest_id = repo
        .derive_hg_changeset(&ctx, child)
        .await?
        .load(&ctx, repo.repo_blobstore())
        .await?
        .manifestid();
    compare_manifests(&ctx, &repo, hg_manifest_id, aug_id).await
}

#[mononoke::fbinit_test]
async fn test_direct_derivation_batch_handles_repeated_merge_acl_root_with_jk(
    fb: FacebookInit,
) -> Result<()> {
    // Given: one direct-derivation batch with two merge commits that share the
    // same non-empty ACL overlay root.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let base = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "restricted/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n",
        )
        .add_file("restricted/base.rs", "fn base() {}")
        .commit()
        .await?;
    let left1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("left1.rs", "fn left1() {}")
        .commit()
        .await?;
    let right1 = CreateCommitContext::new(&ctx, &repo, vec![base])
        .add_file("right1.rs", "fn right1() {}")
        .commit()
        .await?;
    let merge1 = CreateCommitContext::new(&ctx, &repo, vec![left1, right1])
        .commit()
        .await?;
    let left2 = CreateCommitContext::new(&ctx, &repo, vec![merge1])
        .add_file("left2.rs", "fn left2() {}")
        .commit()
        .await?;
    let right2 = CreateCommitContext::new(&ctx, &repo, vec![merge1])
        .add_file("right2.rs", "fn right2() {}")
        .commit()
        .await?;
    let merge2 = CreateCommitContext::new(&ctx, &repo, vec![left2, right2])
        .commit()
        .await?;
    let csids = vec![base, left1, right1, merge1, left2, right2, merge2];
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, csids.clone(), None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
        .await?;
    let acl_roots = manager
        .fetch_derived_batch::<RootAclManifestId>(&ctx, vec![merge1, merge2], None)
        .await?;
    let merge1_acl = derive_hg_augmented_manifest::normalize_acl_root(
        acl_roots.get(&merge1).expect("derived above"),
    )?;
    let merge2_acl = derive_hg_augmented_manifest::normalize_acl_root(
        acl_roots.get(&merge2).expect("derived above"),
    )?;
    assert!(merge1_acl.is_some(), "fixture must have an ACL overlay");
    assert_eq!(merge1_acl, merge2_acl, "fixture must share one ACL root");

    // When: deriving the batch through the direct manager path.
    with_just_knobs_async(
        direct_derivation_knobs(true),
        async {
            manager
                .derive_exactly_batch::<RootHgAugmentedManifestId>(&ctx, csids, None)
                .await
        }
        .boxed(),
    )
    .await?;

    // Then: both merge commits derived through the cached full-ACL-map path
    // still match the Hg manifests.
    for merge in [merge1, merge2] {
        let aug_id = manager
            .fetch_derived::<RootHgAugmentedManifestId>(&ctx, merge, None)
            .await?
            .expect("derived above")
            .hg_augmented_manifest_id();
        let hg_manifest_id = repo
            .derive_hg_changeset(&ctx, merge)
            .await?
            .load(&ctx, repo.repo_blobstore())
            .await?
            .manifestid();
        compare_manifests(&ctx, &repo, hg_manifest_id, aug_id).await?;
    }

    Ok(())
}

/// Verify the full ACL overlay-map cache helper: repeated calls to
/// `cached_acl_overlay_map` with the same `AclManifestId` must reuse the cached
/// `Arc<HashMap<...>>`, not re-walk the tree. This helper is for callers that
/// need a complete ACL path map (for example, later validation tooling); the
/// direct derivation path itself still uses path-scoped overlay loading for
/// non-merge commits.
#[mononoke::fbinit_test]
async fn test_cached_acl_overlay_map_dedupes_within_batch(fb: FacebookInit) -> Result<()> {
    // Given: a non-trivial ACL tree and an initially empty full-overlay cache.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let cs = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "restricted/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n",
        )
        .add_file(
            "restricted/inner/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project2\"\n",
        )
        .add_file("restricted/inner/file.rs", "fn x() {}")
        .add_file("public/readme.md", "hi")
        .commit()
        .await?;
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![cs], None)
        .await?;
    let acl_root = manager
        .fetch_derived::<RootAclManifestId>(&ctx, cs, None)
        .await?
        .expect("derived above");
    let acl_root_overlay = derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;
    assert!(
        acl_root_overlay.is_some(),
        "Test fixture must produce a non-trivial ACL overlay so the cache \
         hit path is actually exercised.",
    );
    let mut cache = HashMap::new();

    // When: asking for the same full ACL overlay map twice.
    let m1 = derive_hg_augmented_manifest::cached_acl_overlay_map(
        &ctx,
        repo.repo_blobstore(),
        acl_root_overlay,
        &mut cache,
    )
    .await?;
    let m2 = derive_hg_augmented_manifest::cached_acl_overlay_map(
        &ctx,
        repo.repo_blobstore(),
        acl_root_overlay,
        &mut cache,
    )
    .await?;

    // Then: the second call returns the cached Arc and does not add another entry.
    assert!(
        Arc::ptr_eq(&m1, &m2),
        "Repeated calls with the same AclManifestId must return the cached \
         Arc, not a fresh pre-walked map. Cache had {} entries; if the cache \
         were broken the second call would still complete but produce a \
         distinct Arc with the same contents.",
        cache.len(),
    );
    assert_eq!(
        cache.len(),
        1,
        "Cache must contain exactly one entry after two calls with the same root.",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_cached_acl_overlay_map_none_does_not_mutate_cache(fb: FacebookInit) -> Result<()> {
    // Given: an existing full-overlay cache entry and a missing ACL root.
    let ctx = CoreContext::test_mock(fb);
    let repo: Repo = test_repo_factory::build_empty(fb).await?;
    let cs = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(
            "restricted/.slacl",
            "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n",
        )
        .add_file("restricted/file.rs", "fn x() {}")
        .commit()
        .await?;
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![cs], None)
        .await?;
    let acl_root = manager
        .fetch_derived::<RootAclManifestId>(&ctx, cs, None)
        .await?
        .expect("derived above");
    let acl_root_overlay = derive_hg_augmented_manifest::normalize_acl_root(&acl_root)?;
    let mut cache = HashMap::new();
    derive_hg_augmented_manifest::cached_acl_overlay_map(
        &ctx,
        repo.repo_blobstore(),
        acl_root_overlay,
        &mut cache,
    )
    .await?;
    assert_eq!(
        cache.len(),
        1,
        "fixture should seed exactly one cache entry"
    );

    // When: asking for a missing ACL root.
    let m_empty = derive_hg_augmented_manifest::cached_acl_overlay_map(
        &ctx,
        repo.repo_blobstore(),
        None,
        &mut cache,
    )
    .await?;

    // Then: the helper returns an empty map without mutating the cache.
    assert!(m_empty.is_empty());
    assert_eq!(cache.len(), 1, "None must not add a cache entry.");

    Ok(())
}
