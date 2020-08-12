/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{IsWarmFn, Warmer, WarmerFn};
use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use consts::HIGHEST_IMPORTED_GEN_NUM;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::{
    compat::Future01CompatExt,
    future::{FutureExt, TryFutureExt},
};
use futures_ext::FutureExt as OldFutureExt;
use futures_old::Future as OldFuture;
use mononoke_types::{ChangesetId, Generation};
use mutable_counters::MutableCounters;
use slog::info;
use std::{sync::Arc, time::Duration};

pub fn blobimport_changeset_warmer(
    ctx: &CoreContext,
    mutable_counters: Arc<dyn MutableCounters>,
) -> Warmer {
    info!(
        ctx.logger(),
        "Warming latest imported changeset in blobimport"
    );
    let warmer: Box<WarmerFn> = {
        let mutable_counters = mutable_counters.clone();
        Box::new(
            move |ctx: CoreContext, repo: BlobRepo, cs_id: ChangesetId| {
                let mutable_counters = mutable_counters.clone();
                async move {
                    let gen_num = fetch_generation_number(&ctx, &repo, cs_id).await?;

                    let duration = Duration::from_secs(1);
                    while !check_if_present_in_hg(&ctx, &mutable_counters, &repo, gen_num).await? {
                        info!(
                            ctx.logger(),
                            "not moving a bookmark to {} because it's not present in hg", cs_id
                        );
                        tokio::time::delay_for(duration).await;
                    }

                    Ok(())
                }
                .boxed()
                .compat()
                .boxify()
            },
        )
    };

    let is_warm: Box<IsWarmFn> = Box::new(
        move |ctx: &CoreContext, repo: &BlobRepo, cs_id: &ChangesetId| {
            let mutable_counters = mutable_counters.clone();
            async move {
                let gen_num = fetch_generation_number(&ctx, &repo, *cs_id).await?;

                check_if_present_in_hg(&ctx, &mutable_counters, &repo, gen_num).await
            }
            .boxed()
        },
    );
    Warmer { warmer, is_warm }
}

async fn fetch_generation_number(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<Generation, Error> {
    let gen_num = repo.get_generation_number(ctx.clone(), cs_id).compat();
    let maybe_gen_num = gen_num.await?;
    maybe_gen_num.ok_or(anyhow!("gen num for {} not found", cs_id))
}

async fn check_if_present_in_hg(
    ctx: &CoreContext,
    mutable_counters: &Arc<dyn MutableCounters>,
    repo: &BlobRepo,
    gen_num: Generation,
) -> Result<bool, Error> {
    let f = mutable_counters
        .get_counter(ctx.clone(), repo.get_repoid(), HIGHEST_IMPORTED_GEN_NUM)
        .compat();
    let maybe_blobimport_gen_num = f.await?;
    if let Some(blobimport_gen_num) = maybe_blobimport_gen_num {
        if blobimport_gen_num >= gen_num.value() as i64 {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn create_derived_data_warmer<D: BonsaiDerived>(ctx: &CoreContext) -> Warmer {
    info!(ctx.logger(), "Warming {}", D::NAME);
    let warmer: Box<WarmerFn> = Box::new(|ctx: CoreContext, repo: BlobRepo, cs_id: ChangesetId| {
        D::derive(ctx, repo, cs_id)
            .map(|_| ())
            .map_err(Error::from)
            .boxify()
    });

    let is_warm: Box<IsWarmFn> =
        Box::new(|ctx: &CoreContext, repo: &BlobRepo, cs_id: &ChangesetId| {
            D::is_derived(&ctx, &repo, &cs_id)
                .map_err(Error::from)
                .boxed()
        });
    Warmer { warmer, is_warm }
}
