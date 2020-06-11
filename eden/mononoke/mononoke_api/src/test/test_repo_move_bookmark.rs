/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use bookmarks::{BookmarkName, BookmarkUpdateReason, Freshness};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures_old::Stream;
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
        BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    )?;
    txn.commit().compat().await?;

    let repo = Repo::new_test(ctx.clone(), blob_repo).await?;
    let repo_ctx = RepoContext::new(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[fbinit::compat_test]
async fn move_bookmark(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;
    let repo = repo.write().await?;

    // Attempt to move from the wrong old target should fail.
    assert!(repo
        .move_bookmark("trunk", changesets["E"], changesets["A"], false)
        .await
        .is_err());

    repo.move_bookmark("trunk", changesets["E"], changesets["C"], false)
        .await?;
    let trunk = repo
        .resolve_bookmark("trunk")
        .await?
        .expect("bookmark should be set");
    assert_eq!(trunk.id(), changesets["E"]);

    // Attempt to move to a non-descendant commit without allowing
    // non-fast-forward moves should fail.
    assert!(repo
        .move_bookmark("trunk", changesets["G"], changesets["E"], false)
        .await
        .is_err());
    repo.move_bookmark("trunk", changesets["G"], changesets["E"], true)
        .await?;
    let trunk = repo
        .resolve_bookmark("trunk")
        .await?
        .expect("bookmark should be set");
    assert_eq!(trunk.id(), changesets["G"]);

    // Check the bookmark moves created BookmarkLogUpdate entries
    let entries = repo
        .blob_repo()
        .get_bookmarks_object()
        .list_bookmark_log_entries(
            ctx.clone(),
            BookmarkName::new("trunk")?,
            RepositoryId::new(0),
            3,
            None,
            Freshness::MostRecent,
        )
        .map(|(cs, rs, _ts)| (cs, rs)) // dropping timestamps
        .collect()
        .compat()
        .await?;
    // TODO(mbthomas): These should have their own BookmarkUpdateReason.
    assert_eq!(
        entries,
        vec![
            (
                Some(changesets["G"]),
                BookmarkUpdateReason::Pushrebase {
                    bundle_replay_data: None
                },
            ),
            (
                Some(changesets["E"]),
                BookmarkUpdateReason::Pushrebase {
                    bundle_replay_data: None
                },
            ),
            (
                Some(changesets["C"]),
                BookmarkUpdateReason::TestMove {
                    bundle_replay_data: None
                },
            ),
        ]
    );

    Ok(())
}
