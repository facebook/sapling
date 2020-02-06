/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use async_trait::async_trait;
use blobstore::Loadable;
use bookmarks::BookmarkTransactionError;
use context::CoreContext;
use fbinit::FacebookInit;
use futures_preview::{compat::Future01CompatExt, future::try_join_all};
use maplit::hashset;
use mercurial_types::globalrev::{Globalrev, START_COMMIT_GLOBALREV};
use mononoke_types::{BonsaiChangesetMut, ChangesetId, RepositoryId};
use pushrebase::{
    do_pushrebase_bonsai, OntoBookmarkParams, PushrebaseCommitHook, PushrebaseHook,
    PushrebaseTransactionHook, RebasedChangesets,
};
use rand::Rng;
use sql::rusqlite::Connection as SqliteConnection;
use sql::Transaction;
use std::time::Duration;
use tests_utils::{bookmark, resolve_cs_id, CreateCommitContext};

use crate::GlobalrevPushrebaseHook;

#[fbinit::test]
async fn pushrebase_assigns_globalrevs(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, _con) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
        SqliteConnection::open_in_memory()?,
        RepositoryId::new(1),
    )?;
    let mapping = repo.bonsai_globalrev_mapping().clone();

    let root = CreateCommitContext::new_root(&ctx, &repo).commit().await?;

    let cs1 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .commit()
        .await?;

    let cs2 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .commit()
        .await?
        .load(ctx.clone(), repo.blobstore())
        .compat()
        .await?;

    let book = bookmark(&ctx, &repo, "master").set_to(cs1).await?;

    let hooks = [GlobalrevPushrebaseHook::new(mapping, repo.get_repoid())];
    let onto = OntoBookmarkParams::new(book);

    let rebased = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &onto,
        &hashset![cs2.clone()],
        &None,
        &hooks,
    )
    .await?
    .rebased_changesets;

    let cs2_rebased = rebased
        .iter()
        .find(|e| e.id_old == cs2.get_changeset_id())
        .ok_or(Error::msg("missing cs2"))?
        .id_new
        .load(ctx.clone(), repo.blobstore())
        .compat()
        .await?;

    let cs3 = CreateCommitContext::new(&ctx, &repo, vec![root])
        .commit()
        .await?
        .load(ctx.clone(), repo.blobstore())
        .compat()
        .await?;

    let cs4 = CreateCommitContext::new(&ctx, &repo, vec![cs3.get_changeset_id()])
        .commit()
        .await?
        .load(ctx.clone(), repo.blobstore())
        .compat()
        .await?;

    let rebased = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &Default::default(),
        &onto,
        &hashset![cs3.clone(), cs4.clone()],
        &None,
        &hooks,
    )
    .await?
    .rebased_changesets;

    let cs3_rebased = rebased
        .iter()
        .find(|e| e.id_old == cs3.get_changeset_id())
        .ok_or(Error::msg("missing cs3"))?
        .id_new
        .load(ctx.clone(), repo.blobstore())
        .compat()
        .await?;

    let cs4_rebased = rebased
        .iter()
        .find(|e| e.id_old == cs4.get_changeset_id())
        .ok_or(Error::msg("missing cs4"))?
        .id_new
        .load(ctx.clone(), repo.blobstore())
        .compat()
        .await?;

    assert_eq!(
        Globalrev::new(START_COMMIT_GLOBALREV),
        Globalrev::from_bcs(&cs2_rebased)?
    );
    assert_eq!(
        Globalrev::new(START_COMMIT_GLOBALREV + 1),
        Globalrev::from_bcs(&cs3_rebased)?
    );
    assert_eq!(
        Globalrev::new(START_COMMIT_GLOBALREV + 2),
        Globalrev::from_bcs(&cs4_rebased)?
    );

    Ok(())
}

#[fbinit::test]
async fn pushrebase_race_assigns_monotonic_globalrevs(fb: FacebookInit) -> Result<(), Error> {
    #[derive(Copy, Clone)]
    struct SleepHook;

    #[async_trait]
    impl PushrebaseHook for SleepHook {
        async fn prepushrebase(&self) -> Result<Box<dyn PushrebaseCommitHook>, Error> {
            let us = rand::thread_rng().gen_range(0, 100);
            tokio::time::delay_for(Duration::from_micros(us)).await;
            Ok(Box::new(*self) as Box<dyn PushrebaseCommitHook>)
        }
    }

    impl PushrebaseCommitHook for SleepHook {
        fn post_rebase_changeset(
            &mut self,
            _bcs_old: ChangesetId,
            _bcs_new: &mut BonsaiChangesetMut,
        ) -> Result<(), Error> {
            Ok(())
        }

        fn into_transaction_hook(
            self: Box<Self>,
            _changesets: &RebasedChangesets,
        ) -> Result<Box<dyn PushrebaseTransactionHook>, Error> {
            Ok(Box::new(*self) as Box<dyn PushrebaseTransactionHook>)
        }
    }

    #[async_trait]
    impl PushrebaseTransactionHook for SleepHook {
        async fn populate_transaction(
            &self,
            _ctx: &CoreContext,
            txn: Transaction,
        ) -> Result<Transaction, BookmarkTransactionError> {
            Ok(txn)
        }
    }

    let ctx = CoreContext::test_mock(fb);
    let (repo, _con) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
        SqliteConnection::open_in_memory()?,
        RepositoryId::new(1),
    )?;
    let mapping = repo.bonsai_globalrev_mapping().clone();

    let root = CreateCommitContext::new_root(&ctx, &repo).commit().await?;
    let book = bookmark(&ctx, &repo, "master").set_to(root).await?;

    let onto = OntoBookmarkParams::new(book);

    let hooks = [
        GlobalrevPushrebaseHook::new(mapping, repo.get_repoid()),
        Box::new(SleepHook) as Box<dyn PushrebaseHook>,
    ];

    let n: u64 = 10;
    let mut futs = vec![];
    for _ in 0..n {
        let bcs = CreateCommitContext::new(&ctx, &repo, vec![root])
            .commit()
            .await?
            .load(ctx.clone(), repo.blobstore())
            .compat()
            .await?;

        let fut = async {
            let params = Default::default();
            let bcss = hashset![bcs];
            do_pushrebase_bonsai(&ctx, &repo, &params, &onto, &bcss, &None, &hooks).await
        };

        futs.push(fut);
    }

    let results = try_join_all(futs).await?;

    // Check that we had some retries
    assert!(results.iter().fold(0, |sum, e| sum + e.retry_num) > 0);

    let mut next_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
    let mut next_globalrev = START_COMMIT_GLOBALREV + n - 1;

    // Check history backwards and assert that globalrevs are going down

    loop {
        if next_cs_id == root {
            break;
        }

        let cs = next_cs_id
            .load(ctx.clone(), repo.blobstore())
            .compat()
            .await?;

        assert_eq!(Globalrev::new(next_globalrev), Globalrev::from_bcs(&cs)?);

        next_cs_id = cs.parents().next().ok_or(Error::msg("no parent found"))?;
        next_globalrev -= 1;
    }

    Ok(())
}
