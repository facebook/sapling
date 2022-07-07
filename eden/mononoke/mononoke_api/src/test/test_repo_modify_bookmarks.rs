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
use bookmarks::Freshness;
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
        changesets["C"],
        BookmarkUpdateReason::TestMove,
    )?;
    txn.commit().await?;

    let repo = Repo::new_test(ctx.clone(), blob_repo).await?;
    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[fbinit::test]
async fn create_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    // Can create public bookmarks on existing changesets (ancestors of trunk).
    repo.create_bookmark("bookmark1", changesets["A"], None)
        .await?;
    let bookmark1 = repo
        .resolve_bookmark("bookmark1", BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(bookmark1.id(), changesets["A"]);

    // Can create public bookmarks on other changesets (not ancestors of trunk).
    repo.create_bookmark("bookmark2", changesets["F"], None)
        .await?;
    let bookmark2 = repo
        .resolve_bookmark("bookmark2", BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(bookmark2.id(), changesets["F"]);

    // Can create scratch bookmarks.
    repo.create_bookmark("scratch/bookmark3", changesets["G"], None)
        .await?;
    let bookmark3 = repo
        .resolve_bookmark("scratch/bookmark3", BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(bookmark3.id(), changesets["G"]);

    // F is now public.  G is not.
    let stack = repo.stack(vec![changesets["G"]], 10).await?;
    assert_eq!(stack.draft, vec![changesets["G"]]);
    assert_eq!(stack.public, vec![changesets["F"]]);

    Ok(())
}

#[fbinit::test]
async fn move_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    repo.move_bookmark("trunk", changesets["E"], None, false, None)
        .await?;
    let trunk = repo
        .resolve_bookmark("trunk", BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(trunk.id(), changesets["E"]);

    // Attempt to move to a non-descendant commit without allowing
    // non-fast-forward moves should fail.
    assert!(
        repo.move_bookmark("trunk", changesets["G"], None, false, None)
            .await
            .is_err()
    );
    repo.move_bookmark("trunk", changesets["G"], None, true, None)
        .await?;
    let trunk = repo
        .resolve_bookmark("trunk", BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(trunk.id(), changesets["G"]);

    // Check the bookmark moves created BookmarkLogUpdate entries
    let entries = repo
        .blob_repo()
        .bookmark_update_log()
        .list_bookmark_log_entries(
            ctx.clone(),
            BookmarkName::new("trunk")?,
            3,
            None,
            Freshness::MostRecent,
        )
        .map_ok(|(_id, cs, rs, _ts)| (cs, rs))
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(
        entries,
        vec![
            (Some(changesets["G"]), BookmarkUpdateReason::ApiRequest),
            (Some(changesets["E"]), BookmarkUpdateReason::ApiRequest),
            (Some(changesets["C"]), BookmarkUpdateReason::TestMove),
        ]
    );

    Ok(())
}

#[fbinit::test]
async fn delete_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    repo.create_bookmark("bookmark1", changesets["A"], None)
        .await?;
    repo.create_bookmark("bookmark2", changesets["F"], None)
        .await?;
    repo.create_bookmark("scratch/bookmark3", changesets["G"], None)
        .await?;

    // Can delete public bookmarks.
    repo.delete_bookmark("bookmark1", None, None).await?;
    assert!(
        repo.resolve_bookmark("bookmark1", BookmarkFreshness::MostRecent)
            .await?
            .is_none()
    );

    // Deleting a bookmark with the wrong old-target fails.
    assert!(
        repo.delete_bookmark("bookmark2", Some(changesets["E"]), None)
            .await
            .is_err()
    );
    let bookmark2 = repo
        .resolve_bookmark("bookmark2", BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(bookmark2.id(), changesets["F"]);

    // But with the right old-target succeeds.
    repo.delete_bookmark("bookmark2", Some(changesets["F"]), None)
        .await?;
    assert!(
        repo.resolve_bookmark("bookmark1", BookmarkFreshness::MostRecent)
            .await?
            .is_none()
    );

    // Deleting a scratch bookmark with the wrong old-target fails.
    assert!(
        repo.delete_bookmark("scratch/bookmark3", Some(changesets["E"]), None)
            .await
            .is_err()
    );

    // But with the right old-target succeeds.
    repo.delete_bookmark("scratch/bookmark3", Some(changesets["G"]), None)
        .await?;
    assert!(
        repo.resolve_bookmark("scratch/bookmark3", BookmarkFreshness::MostRecent)
            .await?
            .is_none()
    );

    Ok(())
}
