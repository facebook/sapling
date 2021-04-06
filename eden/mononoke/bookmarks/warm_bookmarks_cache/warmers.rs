/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{IsWarmFn, Warmer, WarmerFn};
use anyhow::Error;
use blobrepo::BlobRepo;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::future::{FutureExt, TryFutureExt};
use futures_ext::FutureExt as OldFutureExt;
use futures_watchdog::WatchdogExt;
use mononoke_types::ChangesetId;
use slog::{info, o};

pub fn create_derived_data_warmer<D: BonsaiDerived>(ctx: &CoreContext) -> Warmer {
    info!(ctx.logger(), "Warming {}", D::NAME);
    let warmer: Box<WarmerFn> = Box::new(|ctx: CoreContext, repo: BlobRepo, cs_id: ChangesetId| {
        async move {
            D::derive(&ctx, &repo, cs_id).await?;
            Ok(())
        }
        .boxed()
        .compat()
        .boxify()
    });

    let is_warm: Box<IsWarmFn> =
        Box::new(|ctx: &CoreContext, repo: &BlobRepo, cs_id: &ChangesetId| {
            let logger = ctx.logger().new(o!("type" => D::NAME));
            D::is_derived(&ctx, &repo, &cs_id)
                .watched(logger)
                .map_err(Error::from)
                .boxed()
        });
    Warmer { warmer, is_warm }
}
