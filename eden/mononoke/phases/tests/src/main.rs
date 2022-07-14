/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use futures::future;
use futures::TryFutureExt;
use futures::TryStreamExt;

use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use fbinit::FacebookInit;
use fixtures::Linear;
use fixtures::TestRepoFixture;
use maplit::hashset;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::ChangesetId;
use phases::Phases;
use phases::PhasesRef;
use std::str::FromStr;

async fn delete_all_publishing_bookmarks(ctx: &CoreContext, repo: &BlobRepo) -> Result<(), Error> {
    let bookmarks = repo
        .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .try_collect::<Vec<_>>()
        .await?;

    let mut txn = repo.update_bookmark_transaction(ctx.clone());

    for (bookmark, _) in bookmarks {
        txn.force_delete(bookmark.name(), BookmarkUpdateReason::TestMove)
            .unwrap();
    }

    let ok = txn.commit().await?;
    if !ok {
        return Err(Error::msg("Deletion did not commit"));
    }

    Ok(())
}

async fn set_bookmark(
    ctx: &CoreContext,
    repo: &BlobRepo,
    book: &BookmarkName,
    cs_id: &str,
) -> Result<(), Error> {
    let head = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(ctx, HgChangesetId::from_str(cs_id)?)
        .await?
        .ok_or_else(|| Error::msg("cs does not exit"))?;

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(book, head, BookmarkUpdateReason::TestMove)?;

    let ok = txn.commit().await?;
    if !ok {
        return Err(Error::msg("Deletion did not commit"));
    }

    Ok(())
}

async fn is_public(
    phases: &dyn Phases,
    ctx: &CoreContext,
    csid: ChangesetId,
) -> Result<bool, Error> {
    let public = phases.get_public(ctx, vec![csid], false).await?;
    Ok(public.contains(&csid))
}

#[fbinit::test]
async fn get_phase_hint_test(fb: FacebookInit) -> Result<(), Error> {
    let repo = Linear::getrepo(fb).await;
    //  @  79a13814c5ce7330173ec04d279bf95ab3f652fb
    //  |
    //  o  a5ffa77602a066db7d5cfb9fb5823a0895717c5a
    //  |
    //  o  3c15267ebf11807f3d772eb891272b911ec68759
    //  |
    //  o  a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157
    //  |
    //  o  0ed509bf086fadcb8a8a5384dc3b550729b0fc17
    //  |
    //  o  eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b  master
    //  |
    //  o  cb15ca4a43a59acff5388cea9648c162afde8372
    //  |
    //  o  d0a361e9022d226ae52f689667bd7d212a19cfe0
    //  |
    //  o  607314ef579bd2407752361ba1b0c1729d08b281
    //  |
    //  o  3e0e761030db6e479a7fb58b12881883f9f8c63f
    //  |
    //  o  2d7d4ba9ce0a6ffd222de7785b249ead9c51c536

    let ctx = CoreContext::test_mock(fb);

    delete_all_publishing_bookmarks(&ctx, &repo).await?;

    // create a new master bookmark
    set_bookmark(
        &ctx,
        &repo,
        &BookmarkName::new("master").unwrap(),
        "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
    )
    .await?;

    let public_commit = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("d0a361e9022d226ae52f689667bd7d212a19cfe0")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Invalid cs: public_commit"))?;

    let other_public_commit = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Invalid cs: other_public_commit"))?;

    let draft_commit = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Invalid cs: draft_commit"))?;

    let other_draft_commit = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Invalid cs: other_draft_commit"))?;

    let public_bookmark_commit = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(
            &ctx,
            HgChangesetId::from_str("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b")?,
        )
        .await?
        .ok_or_else(|| Error::msg("Invalid cs: public_bookmark_commit"))?;

    let phases = repo.phases();

    assert_eq!(
        is_public(phases, &ctx, public_bookmark_commit).await?,
        true,
        "slow path: get phase for a Public commit which is also a public bookmark"
    );

    assert_eq!(
        is_public(phases, &ctx, public_commit).await?,
        true,
        "slow path: get phase for a Public commit"
    );

    assert_eq!(
        is_public(phases, &ctx, other_public_commit).await?,
        true,
        "slow path: get phase for other Public commit"
    );

    assert_eq!(
        is_public(phases, &ctx, draft_commit).await?,
        false,
        "slow path: get phase for a Draft commit"
    );

    assert_eq!(
        is_public(phases, &ctx, other_draft_commit).await?,
        false,
        "slow path: get phase for other Draft commit"
    );

    let public = phases
        .get_public(
            &ctx,
            vec![
                public_commit,
                other_public_commit,
                draft_commit,
                other_draft_commit,
            ],
            false,
        )
        .await?;

    assert_eq!(
        public,
        hashset! {
            public_commit,
            other_public_commit,
        },
        "slow path: get phases for set of commits"
    );

    assert_eq!(
        is_public(phases, &ctx, public_commit).await?,
        true,
        "sql: make sure that phase was written to the db for public commit"
    );

    assert_eq!(
        is_public(phases, &ctx, draft_commit).await?,
        false,
        "sql: make sure that phase was not written to the db for draft commit"
    );

    Ok(())
}

#[fbinit::test]
async fn test_mark_reachable_as_public(fb: FacebookInit) -> Result<()> {
    let repo = fixtures::BranchEven::getrepo(fb).await;
    // @  4f7f3fd428bec1a48f9314414b063c706d9c1aed (6)
    // |
    // o  b65231269f651cfe784fd1d97ef02a049a37b8a0 (5)
    // |
    // o  d7542c9db7f4c77dab4b315edd328edf1514952f (4)
    // |
    // | o  16839021e338500b3cf7c9b871c8a07351697d68 (3)
    // | |
    // | o  1d8a907f7b4bf50c6a09c16361e2205047ecc5e5 (2)
    // | |
    // | o  3cda5c78aa35f0f5b09780d971197b51cad4613a (1)
    // |/
    // |
    // o  15c40d0abc36d47fb51c8eaec51ac7aad31f669c (0)
    let hgcss = [
        "15c40d0abc36d47fb51c8eaec51ac7aad31f669c",
        "3cda5c78aa35f0f5b09780d971197b51cad4613a",
        "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5",
        "16839021e338500b3cf7c9b871c8a07351697d68",
        "d7542c9db7f4c77dab4b315edd328edf1514952f",
        "b65231269f651cfe784fd1d97ef02a049a37b8a0",
        "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
    ];
    let ctx = CoreContext::test_mock(fb);

    borrowed!(ctx, repo);

    delete_all_publishing_bookmarks(ctx, repo).await?;

    // resolve bonsai
    let bcss = future::try_join_all(
        hgcss
            .iter()
            .map({
                |hgcs| async move {
                    let bcs = repo
                        .bonsai_hg_mapping()
                        .get_bonsai_from_hg(ctx, HgChangesetId::from_str(hgcs)?)
                        .await?
                        .ok_or_else(|| format_err!("Invalid hgcs: {}", hgcs))?;
                    Result::<_, Error>::Ok(bcs)
                }
            })
            .collect::<Vec<_>>(),
    )
    .await?;

    let phases = repo.phases();
    // get phases mapping for all `bcss` in the same order
    let get_phases_map = || {
        phases.get_public(ctx, bcss.clone(), false).map_ok({
            cloned!(bcss);
            move |public| {
                bcss.iter()
                    .map(|bcs| public.contains(bcs))
                    .collect::<Vec<_>>()
            }
        })
    };

    // all phases are draft
    assert_eq!(get_phases_map().await?, [false; 7]);

    phases.add_reachable_as_public(ctx, vec![bcss[1]]).await?;
    assert_eq!(
        get_phases_map().await?,
        [true, true, false, false, false, false, false],
    );

    phases
        .add_reachable_as_public(ctx, vec![bcss[2], bcss[5]])
        .await?;
    assert_eq!(
        get_phases_map().await?,
        [true, true, true, false, true, true, false],
    );

    Ok(())
}
