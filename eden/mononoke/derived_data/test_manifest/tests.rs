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
use bookmarks::BookmarkKey;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use fixtures::TestRepoFixture;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::ManifestOps;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::test_manifest::TestManifestDirectory;
use mononoke_types::test_manifest::TestManifestEntry;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use skeleton_manifest::RootSkeletonManifestId;

use crate::RootTestManifestDirectory;

#[facet::container]
struct Repo(
    dyn BonsaiHgMapping,
    dyn Bookmarks,
    RepoBlobstore,
    RepoDerivedData,
    RepoIdentity,
    CommitGraph,
    dyn CommitGraphWriter,
    FilestoreConfig,
);

/// Validates that the max basename length is calculated correctly for the given directory
/// and all its descendants.
#[async_recursion]
async fn validate(
    visited: &RwLock<HashSet<TestManifestDirectory>>,
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
    dir: TestManifestDirectory,
) -> Result<()> {
    if visited.read().unwrap().contains(&dir) {
        return Ok(());
    }
    visited.write().unwrap().insert(dir.clone());

    let mf = dir.load(ctx, blobstore).await?;
    let calculated_max_basename_length = &AtomicU64::new(0);

    stream::iter(mf.subentries)
        .map(anyhow::Ok)
        .try_for_each_concurrent(None, |(path, entry)| async move {
            match entry {
                TestManifestEntry::File => {
                    calculated_max_basename_length.fetch_max(path.len() as u64, Ordering::Relaxed);
                }
                TestManifestEntry::Directory(dir) => {
                    calculated_max_basename_length
                        .fetch_max(dir.max_basename_length, Ordering::Relaxed);
                    validate(visited, ctx, blobstore, dir).await?;
                }
            }
            Ok(())
        })
        .await?;

    assert_eq!(
        dir.max_basename_length,
        calculated_max_basename_length.load(Ordering::Relaxed)
    );

    Ok(())
}

async fn test_for_fixture<F: TestRepoFixture + Send>(fb: FacebookInit) -> Result<()> {
    let ctx = &CoreContext::test_mock(fb);
    let repo: Repo = F::get_repo(fb).await;
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
    if let Some(master_cs_id) = repo
        .bookmarks()
        .get(
            ctx.clone(),
            &BookmarkKey::new("master").unwrap(),
            bookmarks::Freshness::MostRecent,
        )
        .await?
    {
        futures::future::try_join(
            derived_data.derive::<RootTestManifestDirectory>(ctx, master_cs_id),
            derived_data.derive::<RootSkeletonManifestId>(ctx, master_cs_id),
        )
        .await?;
    }
    let visited = &RwLock::new(HashSet::new());
    for cs_id in all_commits {
        let test_manifest = derived_data
            .derive::<RootTestManifestDirectory>(ctx, cs_id)
            .await?
            .into_inner();
        validate(visited, ctx, blobstore, test_manifest.clone()).await?;

        let skeleton_manifest = derived_data
            .derive::<RootSkeletonManifestId>(ctx, cs_id)
            .await?
            .into_skeleton_manifest_id();

        let test_manifest_leaf_entries = test_manifest
            .list_leaf_entries(ctx.clone(), blobstore.clone())
            .try_collect::<Vec<_>>()
            .await?;
        let skeleton_manifest_leaf_entries = skeleton_manifest
            .list_leaf_entries(ctx.clone(), blobstore.clone())
            .try_collect::<Vec<_>>()
            .await?;
        assert_eq!(test_manifest_leaf_entries, skeleton_manifest_leaf_entries);
    }

    Ok(())
}

macro_rules! define_test {
    ( $test_name:ident, $fixture_name:ty ) => {
        #[mononoke::fbinit_test]
        async fn $test_name(fb: FacebookInit) {
            test_for_fixture::<$fixture_name>(fb).await.unwrap()
        }
    };
}

define_test!(test_manifest_on_linear, fixtures::Linear);
define_test!(test_manifest_on_branch_even, fixtures::BranchEven);
define_test!(test_manifest_on_branch_uneven, fixtures::BranchUneven);
define_test!(test_manifest_on_branch_wide, fixtures::BranchWide);
define_test!(test_manifest_on_merge_even, fixtures::MergeEven);
define_test!(test_manifest_on_many_files_dirs, fixtures::ManyFilesDirs);
define_test!(test_manifest_on_merge_uneven, fixtures::MergeUneven);
define_test!(
    test_manifest_on_unshared_merge_even,
    fixtures::UnsharedMergeEven
);
define_test!(
    test_manifest_on_unshared_merge_uneven,
    fixtures::UnsharedMergeUneven
);
define_test!(test_manifest_on_many_diamonds, fixtures::ManyDiamonds);
