/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Error, Result};
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use futures_ext::{BoxFuture, FutureExt};
use futures_old::{future, Future, Stream};

use bookmarks::{BookmarkName, BookmarkUpdateReason};
use fbinit::FacebookInit;
use fixtures::linear;
use maplit::hashset;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::{ChangesetId, RepositoryId};
use mononoke_types_mocks::changesetid::*;
use phases::{Phase, Phases, SqlPhasesStore};
use sql_construct::SqlConstruct;
use std::str::FromStr;
use std::sync::Arc;
use tokio_compat::runtime::Runtime;

#[fbinit::test]
fn add_get_phase_sql_test(fb: FacebookInit) -> Result<()> {
    let mut rt = Runtime::new()?;
    let ctx = CoreContext::test_mock(fb);
    let repo_id = RepositoryId::new(0);
    let phases = SqlPhasesStore::with_sqlite_in_memory()?;

    rt.block_on(phases.add_public_raw(ctx.clone(), repo_id, vec![ONES_CSID]))?;

    assert_eq!(
        rt.block_on(phases.get_single_raw(repo_id, ONES_CSID))?,
        Some(Phase::Public),
        "sql: get phase for the existing changeset"
    );

    assert_eq!(
        rt.block_on(phases.get_single_raw(repo_id, TWOS_CSID))?,
        None,
        "sql: get phase for non existing changeset"
    );

    assert_eq!(
        rt.block_on(phases.get_public_raw(repo_id, &vec![ONES_CSID, TWOS_CSID]))?,
        hashset! {ONES_CSID},
        "sql: get phase for non existing changeset and existing changeset"
    );

    Ok(())
}

fn delete_all_publishing_bookmarks(rt: &mut Runtime, ctx: CoreContext, repo: BlobRepo) {
    let bookmarks = rt
        .block_on(
            repo.get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
                .collect(),
        )
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx);

    for (bookmark, _) in bookmarks {
        txn.force_delete(
            bookmark.name(),
            BookmarkUpdateReason::TestMove {
                bundle_replay_data: None,
            },
        )
        .unwrap();
    }

    assert!(rt.block_on(txn.commit()).unwrap());
}

fn set_bookmark(
    rt: &mut Runtime,
    ctx: CoreContext,
    repo: BlobRepo,
    book: &BookmarkName,
    cs_id: &str,
) {
    let head = rt
        .block_on(repo.get_bonsai_from_hg(ctx.clone(), HgChangesetId::from_str(cs_id).unwrap()))
        .unwrap()
        .unwrap();
    let mut txn = repo.update_bookmark_transaction(ctx);
    txn.force_set(
        &book,
        head,
        BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    )
    .unwrap();

    assert!(rt.block_on(txn.commit()).unwrap());
}

fn is_public(
    phases: &Arc<dyn Phases>,
    ctx: CoreContext,
    csid: ChangesetId,
) -> BoxFuture<bool, Error> {
    phases
        .get_public(ctx, vec![csid], false)
        .map(move |public| public.contains(&csid))
        .boxify()
}

#[fbinit::test]
fn get_phase_hint_test(fb: FacebookInit) {
    let mut rt = Runtime::new().unwrap();

    let repo = rt.block_on_std(linear::getrepo(fb));
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

    delete_all_publishing_bookmarks(&mut rt, ctx.clone(), repo.clone());

    // create a new master bookmark
    set_bookmark(
        &mut rt,
        ctx.clone(),
        repo.clone(),
        &BookmarkName::new("master").unwrap(),
        "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
    );

    let public_commit = rt
        .block_on(repo.get_bonsai_from_hg(
            ctx.clone(),
            HgChangesetId::from_str("d0a361e9022d226ae52f689667bd7d212a19cfe0").unwrap(),
        ))
        .unwrap()
        .unwrap();

    let other_public_commit = rt
        .block_on(repo.get_bonsai_from_hg(
            ctx.clone(),
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
        ))
        .unwrap()
        .unwrap();

    let draft_commit = rt
        .block_on(repo.get_bonsai_from_hg(
            ctx.clone(),
            HgChangesetId::from_str("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").unwrap(),
        ))
        .unwrap()
        .unwrap();

    let other_draft_commit = rt
        .block_on(repo.get_bonsai_from_hg(
            ctx.clone(),
            HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a").unwrap(),
        ))
        .unwrap()
        .unwrap();

    let public_bookmark_commit = rt
        .block_on(repo.get_bonsai_from_hg(
            ctx.clone(),
            HgChangesetId::from_str("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").unwrap(),
        ))
        .unwrap()
        .unwrap();

    let phases = repo.get_phases();

    assert_eq!(
        rt.block_on(is_public(&phases, ctx.clone(), public_bookmark_commit))
            .unwrap(),
        true,
        "slow path: get phase for a Public commit which is also a public bookmark"
    );

    assert_eq!(
        rt.block_on(is_public(&phases, ctx.clone(), public_commit))
            .unwrap(),
        true,
        "slow path: get phase for a Public commit"
    );

    assert_eq!(
        rt.block_on(is_public(&phases, ctx.clone(), other_public_commit))
            .unwrap(),
        true,
        "slow path: get phase for other Public commit"
    );

    assert_eq!(
        rt.block_on(is_public(&phases, ctx.clone(), draft_commit))
            .unwrap(),
        false,
        "slow path: get phase for a Draft commit"
    );

    assert_eq!(
        rt.block_on(is_public(&phases, ctx.clone(), other_draft_commit))
            .unwrap(),
        false,
        "slow path: get phase for other Draft commit"
    );

    assert_eq!(
        rt.block_on(phases.get_public(
            ctx.clone(),
            vec![
                public_commit,
                other_public_commit,
                draft_commit,
                other_draft_commit
            ],
            false
        ))
        .unwrap(),
        hashset! {
            public_commit,
            other_public_commit,
        },
        "slow path: get phases for set of commits"
    );

    assert_eq!(
        rt.block_on(is_public(&phases, ctx.clone(), public_commit))
            .expect("Get phase failed"),
        true,
        "sql: make sure that phase was written to the db for public commit"
    );

    assert_eq!(
        rt.block_on(is_public(&phases, ctx.clone(), draft_commit))
            .expect("Get phase failed"),
        false,
        "sql: make sure that phase was not written to the db for draft commit"
    );
}

#[fbinit::test]
fn test_mark_reachable_as_public(fb: FacebookInit) -> Result<()> {
    let mut rt = Runtime::new()?;

    let repo = rt.block_on_std(fixtures::branch_even::getrepo(fb));
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

    delete_all_publishing_bookmarks(&mut rt, ctx.clone(), repo.clone());

    // resolve bonsai
    let bcss = rt
        .block_on(future::join_all(
            hgcss
                .iter()
                .map(|hgcs| {
                    repo.get_bonsai_from_hg(ctx.clone(), HgChangesetId::from_str(hgcs).unwrap())
                        .map(|bcs| bcs.unwrap())
                })
                .collect::<Vec<_>>(),
        ))
        .unwrap();

    let phases = repo.get_phases();
    // get phases mapping for all `bcss` in the same order
    let get_phases_map = || {
        phases.get_public(ctx.clone(), bcss.clone(), false).map({
            cloned!(bcss);
            move |public| {
                bcss.iter()
                    .map(|bcs| public.contains(bcs))
                    .collect::<Vec<_>>()
            }
        })
    };

    // all phases are draft
    assert_eq!(rt.block_on(get_phases_map())?, [false; 7]);

    rt.block_on(phases.add_reachable_as_public(ctx.clone(), vec![bcss[1]]))?;
    assert_eq!(
        rt.block_on(get_phases_map())?,
        [true, true, false, false, false, false, false],
    );

    rt.block_on(phases.add_reachable_as_public(ctx.clone(), vec![bcss[2], bcss[5]]))?;
    assert_eq!(
        rt.block_on(get_phases_map())?,
        [true, true, true, false, true, true, false],
    );

    Ok(())
}
