/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use bookmarks::{BookmarkName, BookmarkUpdateLog, BookmarkUpdateReason, Freshness};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use mononoke_types::{ChangesetId, RepositoryId};
use tests_utils::drawdag::create_from_dag;

use crate::repo::{Repo, RepoContext};

async fn init_repo(ctx: &CoreContext) -> Result<(RepoContext, BTreeMap<String, ChangesetId>)> {
    let blob_repo = blobrepo_factory::new_memblob_empty(None)?;
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
        None,
    )?;
    txn.commit().await?;

    let repo = Repo::new_test(ctx.clone(), blob_repo).await?;
    let repo_ctx = RepoContext::new(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[fbinit::compat_test]
async fn move_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;
    let repo = repo.write().await?;

    repo.move_bookmark("trunk", changesets["E"], false).await?;
    let trunk = repo
        .resolve_bookmark("trunk")
        .await?
        .expect("bookmark should be set");
    assert_eq!(trunk.id(), changesets["E"]);

    // Attempt to move to a non-descendant commit without allowing
    // non-fast-forward moves should fail.
    assert!(repo
        .move_bookmark("trunk", changesets["G"], false)
        .await
        .is_err());
    repo.move_bookmark("trunk", changesets["G"], true).await?;
    let trunk = repo
        .resolve_bookmark("trunk")
        .await?
        .expect("bookmark should be set");
    assert_eq!(trunk.id(), changesets["G"]);

    // Check the bookmark moves created BookmarkLogUpdate entries
    let entries = repo
        .blob_repo()
        .attribute_expected::<dyn BookmarkUpdateLog>()
        .list_bookmark_log_entries(
            ctx.clone(),
            BookmarkName::new("trunk")?,
            RepositoryId::new(0),
            3,
            None,
            Freshness::MostRecent,
        )
        .map_ok(|(cs, rs, _ts)| (cs, rs)) // dropping timestamps
        .try_collect::<Vec<_>>()
        .await?;
    // TODO(mbthomas): These should have their own BookmarkUpdateReason.
    assert_eq!(
        entries,
        vec![
            (Some(changesets["G"]), BookmarkUpdateReason::Pushrebase),
            (Some(changesets["E"]), BookmarkUpdateReason::Pushrebase),
            (Some(changesets["C"]), BookmarkUpdateReason::TestMove),
        ]
    );

    Ok(())
}
