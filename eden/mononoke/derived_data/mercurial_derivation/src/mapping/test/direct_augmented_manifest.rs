/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::*;
use crate::RootHgAugmentedManifestV2Id;

async fn load_aug_envelope(
    ctx: &CoreContext,
    repo: &TestRepo,
    aug: &RootHgAugmentedManifestId,
) -> Result<HgAugmentedManifestEnvelope> {
    Loadable::load(&aug.hg_augmented_manifest_id(), ctx, repo.repo_blobstore())
        .await
        .with_context(|| {
            format!(
                "Failed to load augmented manifest envelope for {}",
                aug.hg_augmented_manifest_id(),
            )
        })
}

async fn load_v2_aug_envelope(
    ctx: &CoreContext,
    repo: &TestRepo,
    aug: &RootHgAugmentedManifestV2Id,
) -> Result<HgAugmentedManifestEnvelope> {
    Loadable::load(&aug.hg_augmented_manifest_id(), ctx, repo.repo_blobstore())
        .await
        .with_context(|| {
            format!(
                "Failed to load v2 augmented manifest envelope for {}",
                aug.hg_augmented_manifest_id(),
            )
        })
}

fn assert_aug_envelopes_match(
    old: &HgAugmentedManifestEnvelope,
    new: &HgAugmentedManifestEnvelope,
    cs_id: ChangesetId,
) {
    assert_eq!(
        old.augmented_manifest_id, new.augmented_manifest_id,
        "Blake3 digest mismatch for {cs_id}"
    );
    assert_eq!(
        old.augmented_manifest_size, new.augmented_manifest_size,
        "Size mismatch for {cs_id}"
    );
    assert_eq!(
        old.augmented_manifest.hg_node_id, new.augmented_manifest.hg_node_id,
        "hg_node_id mismatch for {cs_id}"
    );
    assert_eq!(
        old.augmented_manifest.computed_node_id, new.augmented_manifest.computed_node_id,
        "computed_node_id mismatch for {cs_id}"
    );
    assert_eq!(
        old.augmented_manifest.p1, new.augmented_manifest.p1,
        "p1 mismatch for {cs_id}"
    );
    assert_eq!(
        old.augmented_manifest.p2, new.augmented_manifest.p2,
        "p2 mismatch for {cs_id}"
    );
}

async fn create_two_commit_repo(fb: FacebookInit) -> Result<(TestRepo, ChangesetId, ChangesetId)> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let root = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("a.txt", "initial")
        .commit()
        .await?;
    let child = CreateCommitContext::new(&ctx, &repo, vec![root])
        .add_file("a.txt", "modified")
        .add_file("b.txt", "new file")
        .commit()
        .await?;
    Ok((repo, root, child))
}

async fn derive_old_path_augmented_manifest_envelopes(
    ctx: &CoreContext,
    repo: &TestRepo,
    csids: Vec<ChangesetId>,
) -> Result<HashMap<ChangesetId, HgAugmentedManifestEnvelope>> {
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(ctx, csids.clone(), None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(ctx, csids.clone(), None)
        .await?;
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestId>(ctx, csids.clone(), None)
        .await?;
    let roots = manager
        .fetch_derived_batch::<RootHgAugmentedManifestId>(ctx, csids.clone(), None)
        .await?;
    let mut envelopes = HashMap::new();
    for cs_id in csids {
        let aug = roots
            .get(&cs_id)
            .with_context(|| format!("Missing old-path RootHgAugmentedManifestId for {cs_id}"))?;
        envelopes.insert(cs_id, load_aug_envelope(ctx, repo, aug).await?);
    }
    Ok(envelopes)
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest_v2_manager_derivation_does_not_require_hgchangesets(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Given: a linear history with only the v2 non-HgChangeset dependency prederived.
    let (repo, root, child) = create_two_commit_repo(fb).await?;
    let csids = vec![root, child];
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
        .await?;

    // When: deriving v2 through the manager.
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestV2Id>(&ctx, csids.clone(), None)
        .await?;

    // Then: v2 roots are stored and no HgChangeset mappings were created as a side effect.
    let derived = manager
        .fetch_derived_batch::<RootHgAugmentedManifestV2Id>(&ctx, csids.clone(), None)
        .await?;
    for cs_id in csids {
        let aug = derived
            .get(&cs_id)
            .with_context(|| format!("Missing RootHgAugmentedManifestV2Id for {cs_id}"))?;
        let env = load_v2_aug_envelope(&ctx, &repo, aug).await?;
        assert_eq!(
            env.augmented_manifest.hg_node_id, env.augmented_manifest.computed_node_id,
            "v2 should use a content-derived root when no canonical Hg mapping exists for {cs_id}",
        );
        assert!(
            manager
                .fetch_derived::<MappedHgChangesetId>(&ctx, cs_id, None)
                .await?
                .is_none(),
            "v2 manager derivation must not create MappedHgChangesetId for {cs_id}",
        );
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest_v1_direct_batch_requires_hgchangesets(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Given: a linear history with only the ACL dependency prederived.
    let (repo, root, child) = create_two_commit_repo(fb).await?;
    let csids = vec![root, child];
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
        .await?;
    let bonsais = futures::future::try_join_all(
        csids
            .iter()
            .map(|cs_id| Loadable::load(cs_id, &ctx, repo.repo_blobstore())),
    )
    .await?;
    let derivation_ctx = manager.derivation_context(None);

    // When: invoking v1 batch derivation directly below the manager dependency gate.
    let result = RootHgAugmentedManifestId::derive_batch(&ctx, &derivation_ctx, bonsais).await;

    // Then: v1 refuses to derive without its HgChangeset dependency.
    let err = result.expect_err("v1 augmented manifest derivation should require HgChangesets");
    assert!(
        err.to_string().contains("dependency 'hgchangesets'"),
        "unexpected error: {err:#}",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest_v2_derive_single_without_hgchangeset_mapping_matches_old_path(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Given: an old-path baseline and an equivalent repo where only the
    // non-HgChangeset dependency is prederived.
    let (baseline_repo, root, child) = create_two_commit_repo(fb).await?;
    let csids = vec![root, child];
    let baseline_envs =
        derive_old_path_augmented_manifest_envelopes(&ctx, &baseline_repo, csids.clone()).await?;

    let (repo, root, child) = create_two_commit_repo(fb).await?;
    assert_eq!(csids, vec![root, child], "test commits should be stable");
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
        .await?;
    for cs_id in &csids {
        assert!(
            manager
                .fetch_derived::<MappedHgChangesetId>(&ctx, *cs_id, None)
                .await?
                .is_none(),
            "direct single-derive fixture should not prederive MappedHgChangesetId for {cs_id}",
        );
    }
    let derivation_ctx = manager.derivation_context(None);
    let root_bonsai = Loadable::load(&root, &ctx, repo.repo_blobstore()).await?;
    let child_bonsai = Loadable::load(&child, &ctx, repo.repo_blobstore()).await?;

    // When: deriving the root and child through the v2 single-derive path,
    // without a mapped HgChangeset or persisted HgManifest.
    let root_aug = RootHgAugmentedManifestV2Id::derive_single(
        &ctx,
        &derivation_ctx,
        root_bonsai,
        vec![],
        None,
    )
    .await?;
    let child_aug = RootHgAugmentedManifestV2Id::derive_single(
        &ctx,
        &derivation_ctx,
        child_bonsai,
        vec![root_aug.clone()],
        None,
    )
    .await?;

    // Then: v2 single derivation matches the old path and does not create
    // HgChangeset mappings or raw HgManifest blobs.
    for (cs_id, aug) in [(root, root_aug), (child, child_aug)] {
        let env = load_v2_aug_envelope(&ctx, &repo, &aug).await?;
        let baseline = baseline_envs
            .get(&cs_id)
            .with_context(|| format!("Missing baseline envelope for {cs_id}"))?;
        assert_aug_envelopes_match(baseline, &env, cs_id);
        assert_hgmanifest_blob_absent(
            &ctx,
            &repo,
            HgManifestId::new(env.augmented_manifest.hg_node_id),
            cs_id,
        )
        .await?;
        assert!(
            manager
                .fetch_derived::<MappedHgChangesetId>(&ctx, cs_id, None)
                .await?
                .is_none(),
            "direct single derivation must not create MappedHgChangesetId for {cs_id}",
        );
    }

    Ok(())
}

struct NoHgSubtreeCopyFixture {
    repo: TestRepo,
    source: ChangesetId,
    child: ChangesetId,
    csids: Vec<ChangesetId>,
    derivation_ctx: DerivationContext,
    parent_aug: RootHgAugmentedManifestV2Id,
    child_bonsai: BonsaiChangeset,
}

async fn create_no_hg_subtree_copy_fixture(
    ctx: &CoreContext,
    fb: FacebookInit,
) -> Result<NoHgSubtreeCopyFixture> {
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let source = CreateCommitContext::new_root(ctx, &repo)
        .add_file("src/a.txt", "copied content")
        .commit()
        .await?;
    let parent = CreateCommitContext::new_root(ctx, &repo)
        .add_file("base.txt", "base")
        .commit()
        .await?;
    let child = save_bonsai_with_subtree_changes(
        ctx,
        &repo,
        vec![parent],
        vec![subtree_copy("dst", "src", source)?],
    )
    .await?;
    let csids = vec![source, parent, child];
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootAclManifestId>(ctx, csids.clone(), None)
        .await?;
    let derivation_ctx = manager.derivation_context(None);
    let parent_bonsai = Loadable::load(&parent, ctx, repo.repo_blobstore()).await?;
    let child_bonsai = Loadable::load(&child, ctx, repo.repo_blobstore()).await?;
    let parent_aug = RootHgAugmentedManifestV2Id::derive_single(
        ctx,
        &derivation_ctx,
        parent_bonsai,
        vec![],
        None,
    )
    .await?;

    Ok(NoHgSubtreeCopyFixture {
        repo,
        source,
        child,
        csids,
        derivation_ctx,
        parent_aug,
        child_bonsai,
    })
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest_v2_derive_single_uses_known_subtree_source_root_without_hg_mapping(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Given: a no-Hg subtree-copy child whose non-parent source augmented root
    // is available only through the derive_single known cache.
    let NoHgSubtreeCopyFixture {
        repo,
        source,
        child,
        csids,
        derivation_ctx,
        parent_aug,
        child_bonsai,
    } = create_no_hg_subtree_copy_fixture(&ctx, fb).await?;
    let manager = repo.repo_derived_data().manager();
    let source_bonsai = Loadable::load(&source, &ctx, repo.repo_blobstore()).await?;
    let source_aug = RootHgAugmentedManifestV2Id::derive_single(
        &ctx,
        &derivation_ctx,
        source_bonsai,
        vec![],
        None,
    )
    .await?;
    let known = HashMap::from([(source, source_aug)]);

    // When: deriving the subtree-copy child through the v2 single-derive path.
    let child_aug = RootHgAugmentedManifestV2Id::derive_single(
        &ctx,
        &derivation_ctx,
        child_bonsai,
        vec![parent_aug],
        Some(&known),
    )
    .await?;

    // Then: the known source root satisfies the subtree-copy source without
    // creating HgChangeset mappings or repairing the persisted source root.
    let env = load_v2_aug_envelope(&ctx, &repo, &child_aug).await?;
    assert_eq!(
        env.augmented_manifest.hg_node_id, env.augmented_manifest.computed_node_id,
        "v2 should use a content-derived root when no canonical Hg mapping exists for {child}",
    );
    assert!(
        manager
            .fetch_derived::<RootHgAugmentedManifestV2Id>(&ctx, source, None)
            .await?
            .is_none(),
        "derive_single should not repair a missing source augmented root mapping",
    );
    for cs_id in csids {
        assert!(
            manager
                .fetch_derived::<MappedHgChangesetId>(&ctx, cs_id, None)
                .await?
                .is_none(),
            "direct single derivation must not create MappedHgChangesetId for {cs_id}",
        );
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest_v2_derive_single_no_hg_missing_subtree_source_root_errors(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Given: a no-Hg subtree-copy child whose non-parent source augmented root
    // is neither in the `known` cache nor persisted in the shared v2 mapping,
    // with ACL prederived for every changeset so the v2 source-root boundary is
    // what fails (not ACL derivation of the source).
    let NoHgSubtreeCopyFixture {
        repo,
        source,
        csids,
        derivation_ctx,
        parent_aug,
        child_bonsai,
        ..
    } = create_no_hg_subtree_copy_fixture(&ctx, fb).await?;
    let manager = repo.repo_derived_data().manager();

    // When: deriving the subtree-copy child with no known or persisted source root.
    let result = RootHgAugmentedManifestV2Id::derive_single(
        &ctx,
        &derivation_ctx,
        child_bonsai,
        vec![parent_aug],
        None,
    )
    .await;

    // Then: derivation fails with the clear source-root boundary error and does
    // not silently fall back, recursively derive the source, or repair mappings.
    let err = result.expect_err("derivation should require the subtree source root");
    assert!(
        err.to_string().contains("must be derived before"),
        "unexpected error: {err:#}",
    );
    assert!(
        manager
            .fetch_derived::<RootHgAugmentedManifestV2Id>(&ctx, source, None)
            .await?
            .is_none(),
        "failed derivation must not derive or repair the source augmented root",
    );
    for cs_id in csids {
        assert!(
            manager
                .fetch_derived::<MappedHgChangesetId>(&ctx, cs_id, None)
                .await?
                .is_none(),
            "direct single derivation must not create MappedHgChangesetId for {cs_id}",
        );
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest_v2_derive_single_no_hg_uses_persisted_subtree_source_root(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Given: a no-Hg subtree-copy child whose non-parent source augmented root
    // is available only through the shared persisted v2 root mapping (not the
    // `known` cache), with ACL prederived for every changeset.
    let NoHgSubtreeCopyFixture {
        repo,
        source,
        child,
        csids,
        derivation_ctx,
        parent_aug,
        child_bonsai,
    } = create_no_hg_subtree_copy_fixture(&ctx, fb).await?;
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestV2Id>(&ctx, vec![source], None)
        .await?;

    // When: deriving the subtree-copy child with the source root available only
    // via the persisted mapping (the `known` cache is empty).
    let child_aug = RootHgAugmentedManifestV2Id::derive_single(
        &ctx,
        &derivation_ctx,
        child_bonsai,
        vec![parent_aug],
        None,
    )
    .await?;

    // Then: the persisted source root satisfies the subtree copy, the child is
    // content-derived (no canonical Hg mapping), the persisted source mapping is
    // left intact, and no HgChangeset mappings are created.
    let env = load_v2_aug_envelope(&ctx, &repo, &child_aug).await?;
    assert_eq!(
        env.augmented_manifest.hg_node_id, env.augmented_manifest.computed_node_id,
        "v2 should use a content-derived root when no canonical Hg mapping exists for {child}",
    );
    assert!(
        manager
            .fetch_derived::<RootHgAugmentedManifestV2Id>(&ctx, source, None)
            .await?
            .is_some(),
        "the persisted source augmented root mapping should remain available",
    );
    for cs_id in csids {
        assert!(
            manager
                .fetch_derived::<MappedHgChangesetId>(&ctx, cs_id, None)
                .await?
                .is_none(),
            "direct single derivation must not create MappedHgChangesetId for {cs_id}",
        );
    }

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest_v2_derive_single_with_hg_mapping_matches_old_path_for_subtree_copy(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Given: an old-path augmented manifest baseline for a subtree-copy
    // changeset, and an equivalent v2 repo where canonical Hg manifests and
    // ACL manifests are available but no augmented root mapping has been stored.
    let baseline_repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let baseline_source = CreateCommitContext::new_root(&ctx, &baseline_repo)
        .add_file("src/a.txt", "copied content")
        .commit()
        .await?;
    let baseline_child = save_bonsai_with_subtree_changes(
        &ctx,
        &baseline_repo,
        vec![baseline_source],
        vec![subtree_copy("dst", "src", baseline_source)?],
    )
    .await?;
    let csids = vec![baseline_source, baseline_child];
    let baseline_envs =
        derive_old_path_augmented_manifest_envelopes(&ctx, &baseline_repo, csids.clone()).await?;

    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let source = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/a.txt", "copied content")
        .commit()
        .await?;
    let child = save_bonsai_with_subtree_changes(
        &ctx,
        &repo,
        vec![source],
        vec![subtree_copy("dst", "src", source)?],
    )
    .await?;
    assert_eq!(csids, vec![source, child], "test commits should be stable");

    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, csids.clone(), None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
        .await?;
    let derivation_ctx = manager.derivation_context(None);
    let source_bonsai = Loadable::load(&source, &ctx, repo.repo_blobstore()).await?;
    let child_bonsai = Loadable::load(&child, &ctx, repo.repo_blobstore()).await?;
    let source_aug = RootHgAugmentedManifestV2Id::derive_single(
        &ctx,
        &derivation_ctx,
        source_bonsai,
        vec![],
        None,
    )
    .await?;
    assert!(
        manager
            .fetch_derived::<RootHgAugmentedManifestV2Id>(&ctx, source, None)
            .await?
            .is_none(),
        "fixture should not store a v2 augmented root mapping for the subtree source",
    );

    // When: deriving the subtree-copy child through the v2 single-derive path.
    let child_aug = RootHgAugmentedManifestV2Id::derive_single(
        &ctx,
        &derivation_ctx,
        child_bonsai,
        vec![source_aug],
        None,
    )
    .await?;

    // Then: v2 reuses the canonical Hg-manifest result instead of requiring a
    // preexisting augmented root mapping for the subtree source.
    let env = load_v2_aug_envelope(&ctx, &repo, &child_aug).await?;
    let baseline = baseline_envs
        .get(&child)
        .with_context(|| format!("Missing baseline envelope for {child}"))?;
    assert_aug_envelopes_match(baseline, &env, child);

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest_v2_derive_batch_with_hg_mapping_matches_old_path_for_child_only_subtree_copy(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Given: an old-path augmented manifest baseline for a subtree-copy child,
    // and an equivalent v2 repo where the child's parent augmented root and
    // canonical Hg manifests are available but the subtree source has no stored
    // v2 augmented root mapping.
    let baseline_repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let baseline_source = CreateCommitContext::new_root(&ctx, &baseline_repo)
        .add_file("src/a.txt", "copied content")
        .commit()
        .await?;
    let baseline_parent = CreateCommitContext::new_root(&ctx, &baseline_repo)
        .add_file("base.txt", "base")
        .commit()
        .await?;
    let baseline_child = save_bonsai_with_subtree_changes(
        &ctx,
        &baseline_repo,
        vec![baseline_parent],
        vec![subtree_copy("dst", "src", baseline_source)?],
    )
    .await?;
    let csids = vec![baseline_source, baseline_parent, baseline_child];
    let baseline_envs =
        derive_old_path_augmented_manifest_envelopes(&ctx, &baseline_repo, csids.clone()).await?;
    let baseline = baseline_envs
        .get(&baseline_child)
        .with_context(|| format!("Missing baseline envelope for {baseline_child}"))?;

    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let source = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("src/a.txt", "copied content")
        .commit()
        .await?;
    let parent = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("base.txt", "base")
        .commit()
        .await?;
    let child = save_bonsai_with_subtree_changes(
        &ctx,
        &repo,
        vec![parent],
        vec![subtree_copy("dst", "src", source)?],
    )
    .await?;
    assert_eq!(
        csids,
        vec![source, parent, child],
        "test commits should be stable",
    );

    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, csids.clone(), None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, csids.clone(), None)
        .await?;
    manager
        .derive_exactly_batch::<RootHgAugmentedManifestV2Id>(&ctx, vec![parent], None)
        .await?;
    let derivation_ctx = manager.derivation_context(None);
    let child_bonsai = Loadable::load(&child, &ctx, repo.repo_blobstore()).await?;

    // When: deriving only the subtree-copy child through the v2 batch path.
    let derived =
        RootHgAugmentedManifestV2Id::derive_batch(&ctx, &derivation_ctx, vec![child_bonsai])
            .await?;

    // Then: v2 batch derivation reuses the canonical Hg-manifest result instead
    // of requiring a preexisting augmented root mapping for the subtree source.
    let child_aug = derived
        .get(&child)
        .with_context(|| format!("Missing RootHgAugmentedManifestV2Id for {child}"))?;
    let env = load_v2_aug_envelope(&ctx, &repo, child_aug).await?;
    assert_aug_envelopes_match(baseline, &env, child);
    assert!(
        manager
            .fetch_derived::<RootHgAugmentedManifestV2Id>(&ctx, source, None)
            .await?
            .is_none(),
        "fixture should not store a v2 augmented root mapping for the subtree source",
    );

    Ok(())
}

#[mononoke::fbinit_test]
async fn test_augmented_manifest_v2_derive_single_uses_mapped_hg_root_when_available(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, root, _) = create_two_commit_repo(fb).await?;
    let manager = repo.repo_derived_data().manager();

    // Given: the canonical Hg root is available through the Bonsai-Hg
    // mapping, as if a client uploaded a canonical Hg manifest.
    manager
        .derive_exactly_batch::<MappedHgChangesetId>(&ctx, vec![root], None)
        .await?;
    manager
        .derive_exactly_batch::<RootAclManifestId>(&ctx, vec![root], None)
        .await?;
    let mapped_hg = manager
        .fetch_derived::<MappedHgChangesetId>(&ctx, root, None)
        .await?
        .with_context(|| format!("Missing MappedHgChangesetId for {root}"))?;
    let expected_root = Loadable::load(&mapped_hg.hg_changeset_id(), &ctx, repo.repo_blobstore())
        .await?
        .manifestid()
        .into_nodehash();
    let derivation_ctx = manager.derivation_context(None);
    let bonsai = Loadable::load(&root, &ctx, repo.repo_blobstore()).await?;

    // When: deriving through the v2 single-derive path.
    let aug =
        RootHgAugmentedManifestV2Id::derive_single(&ctx, &derivation_ctx, bonsai, vec![], None)
            .await?;

    // Then: the v2 path preserves the mapped canonical Hg root.
    let env = load_v2_aug_envelope(&ctx, &repo, &aug).await?;
    assert_eq!(
        env.augmented_manifest.hg_node_id, expected_root,
        "direct derivation should use the mapped canonical Hg root",
    );

    Ok(())
}
