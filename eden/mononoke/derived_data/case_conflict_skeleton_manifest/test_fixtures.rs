/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::RwLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Result;
use async_recursion::async_recursion;
use blobstore::Blobstore;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use fixtures::TestRepoFixture;
use futures::TryStreamExt;
use itertools::Itertools;
use manifest::ManifestOps;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::case_conflict_skeleton_manifest::CaseConflictSkeletonManifest;
use mononoke_types::case_conflict_skeleton_manifest::CcsmEntry;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use skeleton_manifest::RootSkeletonManifestId;

use crate::derive_from_predecessor::inner_derive_from_predecessor;
use crate::mapping::RootCaseConflictSkeletonManifestId;
use crate::path::CcsmPath;

#[facet::container]
struct TestRepo(
    dyn BonsaiHgMapping,
    dyn Bookmarks,
    CommitGraph,
    dyn CommitGraphWriter,
    RepoDerivedData,
    RepoBlobstore,
    FilestoreConfig,
    RepoIdentity,
);

#[async_recursion]
async fn validate(
    visited: &RwLock<HashSet<CaseConflictSkeletonManifest>>,
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
    manifest: CaseConflictSkeletonManifest,
) -> Result<()> {
    if visited.read().unwrap().contains(&manifest) {
        return Ok(());
    }
    visited.write().unwrap().insert(manifest.clone());

    let calculated_rollup_count = &AtomicU64::new(1);
    let calculated_odd_depth_conflicts = &AtomicU64::new(0);
    let calculated_even_depth_conflicts = &AtomicU64::new(0);

    if manifest.subentries.size() > 1 {
        calculated_even_depth_conflicts.fetch_add(1, Ordering::Relaxed);
    }

    manifest
        .clone()
        .into_subentries(ctx, blobstore)
        .try_for_each_concurrent(None, |(_path, entry)| async move {
            calculated_rollup_count
                .fetch_add(entry.rollup_counts().descendants_count, Ordering::Relaxed);
            calculated_odd_depth_conflicts.fetch_add(
                entry.rollup_counts().even_depth_conflicts,
                Ordering::Relaxed,
            );
            calculated_even_depth_conflicts
                .fetch_add(entry.rollup_counts().odd_depth_conflicts, Ordering::Relaxed);

            match entry {
                CcsmEntry::File => {}
                CcsmEntry::Directory(dir) => validate(visited, ctx, blobstore, dir).await?,
            }
            Ok(())
        })
        .await?;

    let rollup_count = calculated_rollup_count.load(Ordering::Relaxed);
    assert_eq!(manifest.rollup_counts().descendants_count, rollup_count);

    let odd_depth_conflicts = calculated_odd_depth_conflicts.load(Ordering::Relaxed);
    assert_eq!(
        manifest.rollup_counts().odd_depth_conflicts,
        odd_depth_conflicts
    );

    let even_depth_conflicts = calculated_even_depth_conflicts.load(Ordering::Relaxed);
    assert_eq!(
        manifest.rollup_counts().even_depth_conflicts,
        even_depth_conflicts
    );

    Ok(())
}

async fn test_for_fixture<F: TestRepoFixture + Send>(fb: FacebookInit) -> Result<()> {
    let ctx = &CoreContext::test_mock(fb);
    let repo: TestRepo = F::get_repo(fb).await;
    let derived_data = repo.repo_derived_data();
    let blobstore = repo.repo_blobstore();
    let all_commits = repo
        .commit_graph()
        .find_by_prefix(ctx, ChangesetIdPrefix::from_bytes("").unwrap(), 1000)
        .await?;
    let all_commits = match all_commits {
        ChangesetIdsResolvedFromPrefix::Multiple(all_commits) => all_commits,
        other => anyhow::bail!("Unexpected number of commits: {:?}", other),
    };
    let visited = &RwLock::new(HashSet::new());
    repo.commit_graph()
        .process_topologically(ctx, all_commits, |cs_id| async move {
            let ccsm_id = derived_data
                .derive::<RootCaseConflictSkeletonManifestId>(ctx, cs_id)
                .await?;
            let ccsm = ccsm_id.into_inner_id().load(ctx, blobstore).await?;
            validate(visited, ctx, blobstore, ccsm.clone()).await?;

            let skeleton_manifest = derived_data
                .derive::<RootSkeletonManifestId>(ctx, cs_id)
                .await?
                .into_skeleton_manifest_id();

            let skeleton_manifest_leaf_entries = skeleton_manifest
                .list_leaf_entries(ctx.clone(), blobstore.clone())
                .try_collect::<Vec<_>>()
                .await?;

            let ccsm_leaf_entries = ccsm
                .list_leaf_entries(ctx.clone(), blobstore.clone())
                .try_collect::<Vec<_>>()
                .await?;

            assert_eq!(
                ccsm_leaf_entries.into_iter().sorted().collect::<Vec<_>>(),
                skeleton_manifest_leaf_entries
                    .into_iter()
                    .flat_map(
                        |(path, ())| CcsmPath::transform(path).map(|path| (path.into_raw(), ()))
                    )
                    .sorted()
                    .collect::<Vec<_>>(),
            );

            let ccsm_from_skeleton_manifest =
                inner_derive_from_predecessor(ctx, &blobstore.boxed(), skeleton_manifest, 3)
                    .await?;

            assert_eq!(ccsm, ccsm_from_skeleton_manifest);

            Ok(())
        })
        .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_ccsm_repo_fixtures(fb: FacebookInit) {
    futures::try_join!(
        test_for_fixture::<fixtures::Linear>(fb),
        test_for_fixture::<fixtures::BranchEven>(fb),
        test_for_fixture::<fixtures::BranchUneven>(fb),
        test_for_fixture::<fixtures::BranchWide>(fb),
        test_for_fixture::<fixtures::MergeEven>(fb),
        test_for_fixture::<fixtures::ManyFilesDirs>(fb),
        test_for_fixture::<fixtures::MergeUneven>(fb),
        test_for_fixture::<fixtures::UnsharedMergeEven>(fb),
        test_for_fixture::<fixtures::UnsharedMergeUneven>(fb),
        test_for_fixture::<fixtures::ManyDiamonds>(fb),
    )
    .unwrap();
}
