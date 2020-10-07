/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures::try_join;
use mercurial_types::HgChangesetId;
use metaconfig_types::InfinitepushNamespace;
use slog::{info, Logger};
use stats::prelude::*;
use std::sync::Arc;

use crate::errors::ErrorKind;

define_stats! {
    prefix = "mononoke.commitcloud_bookmarks_filler.sync_bookmark";
    update: timeseries(Rate, Sum),
    create: timeseries(Rate, Sum),
    success: timeseries(Rate, Sum),
    failure: timeseries(Rate, Sum),
    total: timeseries(Rate, Sum),
}

pub async fn sync_bookmark(
    fb: FacebookInit,
    blobrepo: BlobRepo,
    logger: Logger,
    infinitepush_namespace: Arc<InfinitepushNamespace>,
    name: BookmarkName,
    hg_cs_id: HgChangesetId,
) -> Result<(), ErrorKind> {
    if !infinitepush_namespace.matches_bookmark(&name) {
        return Err(ErrorKind::InvalidBookmarkForNamespace(name.clone()));
    }

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let (maybe_new_cs_id, maybe_old_cs_id) = try_join!(
        blobrepo.get_bonsai_from_hg(ctx.clone(), hg_cs_id).compat(),
        blobrepo.get_bonsai_bookmark(ctx.clone(), &name).compat()
    )
    .map_err(ErrorKind::BlobRepoError)?;

    let res = async {
        let new_cs_id = match maybe_new_cs_id {
            Some(new_cs_id) => new_cs_id,
            None => return Err(ErrorKind::HgChangesetDoesNotExist(hg_cs_id)),
        };
        cloned!(blobrepo, ctx, logger);
        info!(
            logger,
            "Updating bookmark {:?}: {:?} -> {:?}",
            name.clone(),
            maybe_old_cs_id,
            new_cs_id
        );

        let mut txn = blobrepo.update_bookmark_transaction(ctx);

        match maybe_old_cs_id {
            Some(old_cs_id) => {
                STATS::update.add_value(1);
                txn.update_scratch(&name, new_cs_id, old_cs_id)
            }
            None => {
                STATS::create.add_value(1);
                txn.create_scratch(&name, new_cs_id)
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
