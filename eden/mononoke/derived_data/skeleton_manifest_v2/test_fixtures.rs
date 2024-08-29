/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::RwLock;

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
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2Entry;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use skeleton_manifest::RootSkeletonManifestId;

use crate::derive_from_predecessor::inner_derive_from_predecessor;
use crate::RootSkeletonManifestV2Id;

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
    visited: &RwLock<HashSet<SkeletonManifestV2>>,
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
    manifest: SkeletonManifestV2,
) -> Result<()> {
    if visited.read().unwrap().contains(&manifest) {
        return Ok(());
    }
    visited.write().unwrap().insert(manifest.clone());

    let calculated_rollup_count = AtomicU64::new(1);
    let calculated_rollup_count = &calculated_rollup_count;
    manifest
        .clone()
        .into_subentries(ctx, blobstore)
        .try_for_each_concurrent(None, |(_path, entry)| async move {
            calculated_rollup_count.fetch_add(entry.rollup_count().0, Ordering::Relaxed);
            match entry {
                SkeletonManifestV2Entry::File => {}
                SkeletonManifestV2Entry::Directory(dir) => {
                    validate(visited, ctx, blobstore, dir).await?
                }
            }
            Ok(())
        })
        .await?;
    let count = calculated_rollup_count.load(Ordering::Relaxed);
    assert_eq!(manifest.rollup_count().0, count);

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
            let skeleton_manifest_v2_id = derived_data
                .derive::<RootSkeletonManifestV2Id>(ctx, cs_id)
                .await?;
            let skeleton_manifest_v2 = skeleton_manifest_v2_id
                .into_inner_id()
                .load(ctx, blobstore)
                .await?;
            validate(visited, ctx, blobstore, skeleton_manifest_v2.clone()).await?;

            let skeleton_manifest = derived_data
                .derive::<RootSkeletonManifestId>(ctx, cs_id)
                .await?
                .into_skeleton_manifest_id();

            let skeleton_manifest_v2_leaf_entries = skeleton_manifest_v2
                .list_leaf_entries(ctx.clone(), blobstore.clone())
                .try_collect::<Vec<_>>()
                .await?;
            let skeleton_manifest_leaf_entries = skeleton_manifest
                .list_leaf_entries(ctx.clone(), blobstore.clone())
                .try_collect::<Vec<_>>()
                .await?;

            assert_eq!(
                skeleton_manifest_v2_leaf_entries
                    .into_iter()
                    .sorted()
                    .collect::<Vec<_>>(),
                skeleton_manifest_leaf_entries
                    .into_iter()
                    .sorted()
                    .collect::<Vec<_>>(),
            );

            let skeleton_manifest_v2_from_skeleton_manifest =
                inner_derive_from_predecessor(ctx, &blobstore.boxed(), skeleton_manifest, 3)
                    .await?;

            assert_eq!(
                skeleton_manifest_v2,
                skeleton_manifest_v2_from_skeleton_manifest
            );

            Ok(())
        })
        .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_skeleton_manifest_v2_repo_fixtures(fb: FacebookInit) {
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
