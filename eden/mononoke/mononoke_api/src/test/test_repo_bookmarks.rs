/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use tests_utils::drawdag::create_from_dag;

use crate::repo::BookmarkFreshness;
use crate::repo::Repo;
use crate::repo::RepoContext;

async fn init_repo(ctx: &CoreContext) -> Result<(RepoContext, BTreeMap<String, ChangesetId>)> {
    let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
    let changesets = create_from_dag(
        ctx,
        &repo,
        r"
            A-B-C-D-E
               \
                F-G
        ",
    )
    .await?;
    let mut txn = repo.bookmarks().create_transaction(ctx.clone());
    txn.force_set(
        &BookmarkKey::new("trunk")?,
        changesets["E"],
        BookmarkUpdateReason::TestMove,
    )?;
    txn.create_scratch(&BookmarkKey::new("scratch/branch")?, changesets["G"])?;
    txn.create_scratch(&BookmarkKey::new("scratch/branchpoint")?, changesets["B"])?;
    txn.commit().await?;

    repo.warm_bookmarks_cache().sync(ctx).await;

    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[fbinit::test]
async fn resolve_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    // Test that normal bookmarks are visible both in and through the cache.
    assert_eq!(
        repo.resolve_bookmark(&BookmarkKey::new("trunk")?, BookmarkFreshness::MostRecent)
            .await?
            .unwrap()
            .id(),
        changesets["E"],
    );

    assert_eq!(
        repo.resolve_bookmark(&BookmarkKey::new("trunk")?, BookmarkFreshness::MaybeStale)
            .await?
            .unwrap()
            .id(),
        changesets["E"],
    );

    // Test that scratch bookmarks are visible through the cache.
    assert_eq!(
        repo.resolve_bookmark(
            &BookmarkKey::new("scratch/branch")?,
            BookmarkFreshness::MaybeStale
        )
        .await?
        .unwrap()
        .id(),
        changesets["G"],
    );

    assert_eq!(
        repo.resolve_bookmark(
            &BookmarkKey::new("scratch/branchpoint")?,
            BookmarkFreshness::MaybeStale
        )
        .await?
        .unwrap()
        .id(),
        changesets["B"],
    );

    // Test that non-existent bookmarks don't exist either way.
    assert!(
        repo.resolve_bookmark(
            &BookmarkKey::new("scratch/nonexistent")?,
            BookmarkFreshness::MaybeStale
        )
        .await?
        .is_none()
    );

    assert!(
        repo.resolve_bookmark(
            &BookmarkKey::new("nonexistent")?,
            BookmarkFreshness::MostRecent
        )
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
