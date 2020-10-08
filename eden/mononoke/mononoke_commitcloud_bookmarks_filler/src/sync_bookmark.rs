/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::replay_stream::ReplayFn;
use anyhow::format_err;
use async_trait::async_trait;
use bookmarks::BookmarkName;
use cloned::cloned;
use fbinit::FacebookInit;
use futures::try_join;
use mercurial_types::HgChangesetId;
use mononoke_api::{BookmarkFreshness, ChangesetSpecifier, CoreContext, Mononoke};
use slog::{info, Logger};
use stats::prelude::*;

use crate::errors::ErrorKind;

define_stats! {
    prefix = "mononoke.commitcloud_bookmarks_filler.sync_bookmark";
    update: timeseries(Rate, Sum),
    create: timeseries(Rate, Sum),
    success: timeseries(Rate, Sum),
    failure: timeseries(Rate, Sum),
    total: timeseries(Rate, Sum),
}

pub struct SyncBookmark<'a, 'b, 'c> {
    fb: &'a FacebookInit,
    mononoke: &'b Mononoke,
    logger: &'c Logger,
}

impl<'a, 'b, 'c> SyncBookmark<'a, 'b, 'c> {
    pub fn new(fb: &'a FacebookInit, mononoke: &'b Mononoke, logger: &'c Logger) -> Self {
        SyncBookmark {
            fb,
            mononoke,
            logger,
        }
    }
}

#[async_trait]
impl<'a, 'b, 'c> ReplayFn for &SyncBookmark<'a, 'b, 'c> {
    async fn replay(
        &self,
        repo_name: String,
        bookmark_name: BookmarkName,
        hg_cs_id: HgChangesetId,
    ) -> Result<(), ErrorKind> {
        let ctx = CoreContext::new_with_logger(self.fb.clone(), self.logger.clone());

        let repo = self
            .mononoke
            .repo(ctx.clone(), &repo_name)
            .await
            .map_err(|e| ErrorKind::BlobRepoError(e.into()))?
            .ok_or_else(|| format_err!("repo doesn't exist: {:?}", repo_name))
            .map_err(ErrorKind::BlobRepoError)?;

        let infinitepush_namespace = repo
            .config()
            .infinitepush
            .namespace
            .as_ref()
            .ok_or_else(|| format_err!("Infinitepush is not enabled in repository {:?}", repo_name))
            .map_err(ErrorKind::BlobRepoError)?;

        if !infinitepush_namespace.matches_bookmark(&bookmark_name) {
            return Err(ErrorKind::InvalidBookmarkForNamespace(
                bookmark_name.clone(),
            ));
        }


        let (maybe_new_cs_id, maybe_old_cs) = try_join!(
            repo.resolve_specifier(ChangesetSpecifier::Hg(hg_cs_id)),
            repo.resolve_bookmark(bookmark_name.as_str(), BookmarkFreshness::MostRecent)
        )
        .map_err(|e| ErrorKind::BlobRepoError(e.into()))?;

        let maybe_old_cs_id = maybe_old_cs.map(|cs| cs.id());

        let res = async {
            let new_cs_id = match maybe_new_cs_id {
                Some(new_cs_id) => new_cs_id,
                None => return Err(ErrorKind::HgChangesetDoesNotExist(hg_cs_id)),
            };
            cloned!(ctx, self.logger);
            info!(
                logger,
                "Updating repo: {:?} {:?}: {:?} -> {:?}",
                repo_name.clone(),
                bookmark_name.clone(),
                maybe_old_cs_id,
                new_cs_id
            );

            let blobrepo = repo.blob_repo();
            let mut txn = blobrepo.update_bookmark_transaction(ctx);

            match maybe_old_cs_id {
                Some(old_cs_id) => {
                    STATS::update.add_value(1);
                    txn.update_scratch(&bookmark_name, new_cs_id, old_cs_id)
                }
                None => {
                    STATS::create.add_value(1);
                    txn.create_scratch(&bookmark_name, new_cs_id)
                }
            }
            .map_err(ErrorKind::BlobRepoError)?;

            let success = txn.commit().await.map_err(ErrorKind::BlobRepoError)?;

            if !success {
                return Err(ErrorKind::BookmarkTransactionFailed);
            }
            Ok(())
        }
        .await;
        STATS::total.add_value(1);

        if res.is_ok() {
            STATS::success.add_value(1);
        } else {
            STATS::failure.add_value(1);
        }
        res
    }
}
