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
use futures::future::TryFutureExt;
use futures_ext::FutureExt;
use futures_old::future::{self, Future};
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

pub fn sync_bookmark(
    fb: FacebookInit,
    blobrepo: BlobRepo,
    logger: Logger,
    infinitepush_namespace: Arc<InfinitepushNamespace>,
    name: &BookmarkName,
    hg_cs_id: &HgChangesetId,
) -> impl Future<Item = (), Error = ErrorKind> {
    if !infinitepush_namespace.matches_bookmark(name) {
        return future::err(ErrorKind::InvalidBookmarkForNamespace(name.clone())).left_future();
    }

    cloned!(name, hg_cs_id);

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let maybe_new_cs_id = blobrepo.get_bonsai_from_hg(ctx.clone(), hg_cs_id);
    let maybe_old_cs_id = blobrepo.get_bonsai_bookmark(ctx.clone(), &name);

    maybe_old_cs_id
        .join(maybe_new_cs_id)
        .map_err(ErrorKind::BlobRepoError)
        .and_then(move |(maybe_old_cs_id, maybe_new_cs_id)| {
            match maybe_new_cs_id {
                Some(new_cs_id) => Ok((maybe_old_cs_id, new_cs_id)),
                None => Err(ErrorKind::HgChangesetDoesNotExist(hg_cs_id)),
            }
        })
        .and_then({
            cloned!(blobrepo, ctx, logger);
            move |(maybe_old_cs_id, new_cs_id)| {
                info!(
                    logger,
                    "Updating bookmark {:?}: {:?} -> {:?}", name, maybe_old_cs_id, new_cs_id
                );

                let mut txn = blobrepo.update_bookmark_transaction(ctx);

                let res = match maybe_old_cs_id {
                    Some(old_cs_id) => {
                        STATS::update.add_value(1);
                        txn.update_scratch(&name, new_cs_id, old_cs_id)
                    }
                    None => {
                        STATS::create.add_value(1);
                        txn.create_scratch(&name, new_cs_id)
                    }
                };

                res.map(|_| txn).map_err(ErrorKind::BlobRepoError)
            }
        })
        .and_then(|txn| txn.commit().compat().map_err(ErrorKind::BlobRepoError))
        .and_then(|success| {
            if success {
                future::ok(())
            } else {
                future::err(ErrorKind::BookmarkTransactionFailed)
            }
        })
        .inspect_result(|res| {
            STATS::total.add_value(1);

            if res.is_ok() {
                STATS::success.add_value(1);
            } else {
                STATS::failure.add_value(1);
            }
        })
        .right_future()
}
