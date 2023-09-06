/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
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
        changesets["C"],
        BookmarkUpdateReason::TestMove,
    )?;
    txn.commit().await?;

    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[fbinit::test]
async fn create_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    // Can create public bookmarks on existing changesets (ancestors of trunk).
    let key = BookmarkKey::new("bookmark1")?;
    repo.create_bookmark(&key, changesets["A"], None).await?;
    let bookmark1 = repo
        .resolve_bookmark(&key, BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(bookmark1.id(), changesets["A"]);

    // Can create public bookmarks on other changesets (not ancestors of trunk).
    let key = BookmarkKey::new("bookmark2")?;
    repo.create_bookmark(&key, changesets["F"], None).await?;
    let bookmark2 = repo
        .resolve_bookmark(&key, BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(bookmark2.id(), changesets["F"]);

    // Can create scratch bookmarks.
    let key = BookmarkKey::new("scratch/bookmark3")?;
    repo.create_bookmark(&key, changesets["G"], None).await?;
    let bookmark3 = repo
        .resolve_bookmark(&key, BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(bookmark3.id(), changesets["G"]);

    // Can create tag bookmark
    let key =
        BookmarkKey::with_name_and_category(BookmarkName::new("tag1")?, BookmarkCategory::Tag);
    repo.create_bookmark(&key, changesets["B"], None).await?;
    let tag = repo
        .resolve_bookmark(&key, BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(tag.id(), changesets["B"]);

    // Can create note bookmark
    let key =
        BookmarkKey::with_name_and_category(BookmarkName::new("note1")?, BookmarkCategory::Note);
    repo.create_bookmark(&key, changesets["D"], None).await?;
    let note = repo
        .resolve_bookmark(&key, BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(note.id(), changesets["D"]);

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

    let key = BookmarkKey::new("trunk")?;
    repo.move_bookmark(&key, changesets["E"], None, false, None)
        .await?;
    let trunk = repo
        .resolve_bookmark(&key, BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(trunk.id(), changesets["E"]);

    // Attempt to move to a non-descendant commit without allowing
    // non-fast-forward moves should fail.
    assert!(
        repo.move_bookmark(&key, changesets["G"], None, false, None)
            .await
            .is_err()
    );
    repo.move_bookmark(&key, changesets["G"], None, true, None)
        .await?;
    let trunk = repo
        .resolve_bookmark(&key, BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(trunk.id(), changesets["G"]);

    // Check the bookmark moves created BookmarkLogUpdate entries
    let entries = repo
        .blob_repo()
        .bookmark_update_log()
        .list_bookmark_log_entries(ctx.clone(), key, 3, None, Freshness::MostRecent)
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

    // Tags can also be moved
    let key =
        BookmarkKey::with_name_and_category(BookmarkName::new("tag1")?, BookmarkCategory::Tag);
    repo.create_bookmark(&key, changesets["C"], None).await?;
    repo.move_bookmark(&key, changesets["D"], None, false, None)
        .await?;
    let entries = repo
        .blob_repo()
        .bookmark_update_log()
        .list_bookmark_log_entries(ctx.clone(), key, 1, None, Freshness::MostRecent)
        .map_ok(|(_id, cs, rs, _ts)| (cs, rs))
        .try_collect::<Vec<_>>()
        .await?;
    assert_eq!(
        entries,
        vec![(Some(changesets["D"]), BookmarkUpdateReason::ApiRequest),]
    );

    Ok(())
}

#[fbinit::test]
async fn delete_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    let bookmark1_key = BookmarkKey::new("bookmark1")?;
    repo.create_bookmark(&bookmark1_key, changesets["A"], None)
        .await?;
    let bookmark2_key = BookmarkKey::new("bookmark2")?;
    repo.create_bookmark(&bookmark2_key, changesets["F"], None)
        .await?;
    let bookmark3_key = BookmarkKey::new("scratch/bookmark3")?;
    repo.create_bookmark(&bookmark3_key, changesets["G"], None)
        .await?;
    let tag_key =
        BookmarkKey::with_name_and_category(BookmarkName::new("tag1")?, BookmarkCategory::Tag);
    repo.create_bookmark(&tag_key, changesets["B"], None)
        .await?;
    let note_key =
        BookmarkKey::with_name_and_category(BookmarkName::new("note1")?, BookmarkCategory::Note);
    repo.create_bookmark(&note_key, changesets["D"], None)
        .await?;

    // Can delete public bookmarks (of any category).
    repo.delete_bookmark(&bookmark1_key, None, None).await?;
    assert!(
        repo.resolve_bookmark(&bookmark1_key, BookmarkFreshness::MostRecent)
            .await?
            .is_none()
    );
    repo.delete_bookmark(&tag_key, None, None).await?;
    assert!(
        repo.resolve_bookmark(&tag_key, BookmarkFreshness::MostRecent)
            .await?
            .is_none()
    );
    repo.delete_bookmark(&note_key, None, None).await?;
    assert!(
        repo.resolve_bookmark(&note_key, BookmarkFreshness::MostRecent)
            .await?
            .is_none()
    );

    // Deleting a bookmark with the wrong old-target fails.
    assert!(
        repo.delete_bookmark(&bookmark2_key, Some(changesets["E"]), None)
            .await
            .is_err()
    );
    let bookmark2 = repo
        .resolve_bookmark(&bookmark2_key, BookmarkFreshness::MostRecent)
        .await?
        .expect("bookmark should be set");
    assert_eq!(bookmark2.id(), changesets["F"]);

    // But with the right old-target succeeds.
    repo.delete_bookmark(&bookmark2_key, Some(changesets["F"]), None)
        .await?;
    assert!(
        repo.resolve_bookmark(&bookmark1_key, BookmarkFreshness::MostRecent)
            .await?
            .is_none()
    );

    // Deleting a scratch bookmark with the wrong old-target fails.
    assert!(
        repo.delete_bookmark(&bookmark3_key, Some(changesets["E"]), None)
            .await
            .is_err()
    );

    // But with the right old-target succeeds.
    repo.delete_bookmark(&bookmark3_key, Some(changesets["G"]), None)
        .await?;
    assert!(
        repo.resolve_bookmark(&bookmark3_key, BookmarkFreshness::MostRecent)
            .await?
            .is_none()
    );

    Ok(())
}
