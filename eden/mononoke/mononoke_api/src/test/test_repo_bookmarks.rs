/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use tests_utils::drawdag::create_from_dag;

use crate::repo::BookmarkFreshness;
use crate::repo::Repo;
use crate::repo::RepoContext;

async fn init_repo(ctx: &CoreContext) -> Result<(RepoContext, BTreeMap<String, ChangesetId>)> {
    let blob_repo: BlobRepo = test_repo_factory::build_empty(ctx.fb)?;
    let changesets = create_from_dag(
        ctx,
        &blob_repo,
        r##"
            A-B-C-D-E
               \
                F-G
        "##,
    )
    .await?;
    let mut txn = blob_repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(
        &BookmarkName::new("trunk")?,
        changesets["E"],
        BookmarkUpdateReason::TestMove,
    )?;
    txn.create_scratch(&BookmarkName::new("scratch/branch")?, changesets["G"])?;
    txn.create_scratch(&BookmarkName::new("scratch/branchpoint")?, changesets["B"])?;
    txn.commit().await?;

    let repo = Repo::new_test(ctx.clone(), blob_repo).await?;
    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[fbinit::test]
async fn resolve_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    // Test that normal bookmarks are visible both in and through the cache.
    assert_eq!(
        repo.resolve_bookmark("trunk", BookmarkFreshness::MostRecent)
            .await?
            .unwrap()
            .id(),
        changesets["E"],
    );

    assert_eq!(
        repo.resolve_bookmark("trunk", BookmarkFreshness::MaybeStale)
            .await?
            .unwrap()
            .id(),
        changesets["E"],
    );

    // Test that scratch bookmarks are visible through the cache.
    assert_eq!(
        repo.resolve_bookmark("scratch/branch", BookmarkFreshness::MaybeStale)
            .await?
            .unwrap()
            .id(),
        changesets["G"],
    );

    assert_eq!(
        repo.resolve_bookmark("scratch/branchpoint", BookmarkFreshness::MaybeStale)
            .await?
            .unwrap()
            .id(),
        changesets["B"],
    );

    // Test that non-existent bookmarks don't exist either way.
    assert!(
        repo.resolve_bookmark("scratch/nonexistent", BookmarkFreshness::MaybeStale)
            .await?
            .is_none()
    );

    assert!(
        repo.resolve_bookmark("nonexistent", BookmarkFreshness::MostRecent)
            .await?
            .is_none()
    );

    Ok(())
}

#[fbinit::test]
async fn list_bookmarks(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    assert_eq!(
        repo.list_bookmarks(false, None, None, None)
            .await?
            .try_collect::<Vec<_>>()
            .await?,
        vec![(String::from("trunk"), changesets["E"])]
    );

    assert_eq!(
        repo.list_bookmarks(true, Some("scratch/"), None, Some(3))
            .await?
            .try_collect::<Vec<_>>()
            .await?,
        vec![
            (String::from("scratch/branch"), changesets["G"]),
            (String::from("scratch/branchpoint"), changesets["B"])
        ]
    );

    assert_eq!(
        repo.list_bookmarks(true, Some("scratch/"), Some("scratch/branch"), Some(3))
            .await?
            .try_collect::<Vec<_>>()
            .await?,
        vec![(String::from("scratch/branchpoint"), changesets["B"])]
    );
    Ok(())
}
