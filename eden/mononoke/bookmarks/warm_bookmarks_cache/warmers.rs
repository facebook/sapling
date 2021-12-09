/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{IsWarmFn, Warmer, WarmerFn};
use anyhow::Error;
use context::CoreContext;
use derived_data::BonsaiDerived;
use derived_data_manager::BonsaiDerivable;
use futures::future::{FutureExt, TryFutureExt};
use futures_ext::FutureExt as OldFutureExt;
use futures_watchdog::WatchdogExt;
use mononoke_api_types::InnerRepo;
use mononoke_types::ChangesetId;
use phases::{Phases, PhasesRef};
use slog::{info, o};

pub fn create_derived_data_warmer<D: BonsaiDerivable + BonsaiDerived>(ctx: &CoreContext) -> Warmer {
    info!(ctx.logger(), "Warming {}", D::DERIVABLE_NAME);
    let warmer: Box<WarmerFn> =
        Box::new(|ctx: CoreContext, repo: InnerRepo, cs_id: ChangesetId| {
            async move {
                D::derive(&ctx, &repo.blob_repo, cs_id).await?;
                Ok(())
            }
            .boxed()
            .compat()
            .boxify()
        });

    let is_warm: Box<IsWarmFn> =
        Box::new(|ctx: &CoreContext, repo: &InnerRepo, cs_id: &ChangesetId| {
            let logger = ctx.logger().new(o!("type" => D::DERIVABLE_NAME));
            D::is_derived(&ctx, &repo.blob_repo, &cs_id)
                .watched(logger)
                .map_err(Error::from)
                .boxed()
        });
    Warmer {
        warmer,
        is_warm,
        name: D::NAME.to_string(),
    }
}

pub fn create_public_phase_warmer(ctx: &CoreContext) -> Warmer {
    info!(ctx.logger(), "Warming public phases");
    let warmer: Box<WarmerFn> =
        Box::new(|ctx: CoreContext, repo: InnerRepo, cs_id: ChangesetId| {
            async move {
                repo.blob_repo
                    .phases()
                    .add_reachable_as_public(ctx, vec![cs_id])
                    .await?;
                Ok(())
            }
            .boxed()
            .compat()
            .boxify()
        });

    let is_warm: Box<IsWarmFn> =
        Box::new(|ctx: &CoreContext, repo: &InnerRepo, cs_id: &ChangesetId| {
            async move {
                let maybe_public = repo
                    .blob_repo
                    .phases()
                    .get_store()
                    .get_public(ctx.clone(), vec![*cs_id], false /* ephemeral derive */)
                    .await?;

                Ok(maybe_public.contains(cs_id))
            }
            .boxed()
        });
    Warmer {
        warmer,
        is_warm,
        name: "public phases".to_string(),
    }
}
