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

                    let log_rate = 60;
                    let mut i = 0;
                    let duration = Duration::from_secs(1);
                    while !check_if_present_in_hg(&ctx, &mutable_counters, &repo, gen_num).await? {
                        i += 1;
                        if i % log_rate == 0 {
                            info!(
                                ctx.logger(),
                                "not moving a bookmark to {} because it's not present in hg", cs_id
                            );
                        }
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
    repo.get_generation_number(ctx.clone(), cs_id)
        .await?
        .ok_or_else(|| anyhow!("gen num for {} not found", cs_id))
}

async fn check_if_present_in_hg(
    ctx: &CoreContext,
    mutable_counters: &Arc<dyn MutableCounters>,
    repo: &BlobRepo,
    gen_num: Generation,
) -> Result<bool, Error> {
    // Prefer reading stale counter (i.e. from replica) to avoid overloading sql
    // leader
    let f = mutable_counters
        .get_maybe_stale_counter(ctx.clone(), repo.get_repoid(), HIGHEST_IMPORTED_GEN_NUM)
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
            D::is_derived(&ctx, &repo, &cs_id)
                .map_err(Error::from)
                .boxed()
        });
    Warmer { warmer, is_warm }
}
