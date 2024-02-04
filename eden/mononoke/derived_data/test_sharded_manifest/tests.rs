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
use commit_graph::CommitGraphRef;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::TestRepoFixture;
use futures::stream;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::test_sharded_manifest::TestShardedManifestDirectory;
use mononoke_types::test_sharded_manifest::TestShardedManifestEntry;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use skeleton_manifest::RootSkeletonManifestId;
use test_manifest::RootTestManifestDirectory;

use crate::derive_from_predecessor::inner_derive_from_predecessor;
use crate::RootTestShardedManifestDirectory;

/// Validates that the max basename length is calculated correctly for the given directory
/// and all its descendants.
#[async_recursion]
async fn validate(
    visited: &RwLock<HashSet<TestShardedManifestDirectory>>,
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
    dir: TestShardedManifestDirectory,
) -> Result<()> {
    if visited.read().unwrap().contains(&dir) {
        return Ok(());
    }
    visited.write().unwrap().insert(dir.clone());

    let mf = dir.load(ctx, blobstore).await?;
    let calculated_max_basename_length = &AtomicU64::new(0);

    mf.into_subentries(ctx, blobstore)
        .try_for_each_concurrent(None, |(_path, entry)| async move {
            match entry {
                TestShardedManifestEntry::File(file) => {
                    calculated_max_basename_length
                        .fetch_max(file.basename_length, Ordering::Relaxed);
                }
                TestShardedManifestEntry::Directory(dir) => {
                    calculated_max_basename_length
                        .fetch_max(dir.max_basename_length.into_inner(), Ordering::Relaxed);
                    validate(visited, ctx, blobstore, dir).await?;
                }
            }
            Ok(())
        })
        .await?;

    assert_eq!(
        dir.max_basename_length.into_inner(),
        calculated_max_basename_length.load(Ordering::Relaxed)
    );

    Ok(())
}

async fn test_for_fixture<F: TestRepoFixture + Send>(fb: FacebookInit) -> Result<()> {
    let ctx = &CoreContext::test_mock(fb);
    let repo = F::getrepo(fb).await;
    let derived_data = repo.repo_derived_data();
    let blobstore = repo.repo_blobstore();
    let all_commits = repo
        .commit_graph()
        .find_by_prefix(ctx, ChangesetIdPrefix::from_bytes("").unwrap(), 1000)
        .await?;
    let all_commits = match all_commits {
        ChangesetIdsResolvedFromPrefix::Multiple(all_commits) => all_commits,
        other => anyhow::bail!("Weird number of commits: {:?}", other),
    };
    let visited = &RwLock::new(HashSet::new());
    stream::iter(all_commits.into_iter().map(anyhow::Ok))
        .try_for_each_concurrent(None, |cs_id| async move {
            let test_sharded_manifest = derived_data
                .derive::<RootTestShardedManifestDirectory>(ctx, cs_id)
                .await?
                .into_inner();
            validate(visited, ctx, blobstore, test_sharded_manifest.clone()).await?;

            let skeleton_manifest = derived_data
                .derive::<RootSkeletonManifestId>(ctx, cs_id)
                .await?
                .into_skeleton_manifest_id();

            let test_sharded_manifest_leaf_entries = test_sharded_manifest
                .list_leaf_entries(ctx.clone(), blobstore.clone())
                .try_collect::<Vec<_>>()
                .await?;
            let skeleton_manifest_leaf_entries = skeleton_manifest
                .list_leaf_entries(ctx.clone(), blobstore.clone())
                .try_collect::<Vec<_>>()
                .await?;

            assert_eq!(
                test_sharded_manifest_leaf_entries,
                skeleton_manifest_leaf_entries
            );

            let test_manifest = derived_data
                .derive::<RootTestManifestDirectory>(ctx, cs_id)
                .await?
                .into_inner();
            let test_sharded_manifest_from_test_manifest =
                inner_derive_from_predecessor(ctx, &blobstore.boxed(), test_manifest).await?;

            assert_eq!(
                test_sharded_manifest,
                test_sharded_manifest_from_test_manifest
            );

            Ok(())
        })
        .await?;
    Ok(())
}

#[fbinit::test]
async fn test_sharded_manifest_on_repo_fixtures(fb: FacebookInit) {
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
