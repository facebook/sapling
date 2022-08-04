/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cloned::cloned;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use futures::future::FutureExt;
use futures_watchdog::WatchdogExt;
use mononoke_types::ChangesetId;
use phases::ArcPhases;
use repo_derived_data::ArcRepoDerivedData;
use slog::info;
use slog::o;

use super::IsWarmFn;
use super::Warmer;
use super::WarmerFn;

pub fn create_derived_data_warmer<Derivable>(
    ctx: &CoreContext,
    repo_derived_data: ArcRepoDerivedData,
) -> Warmer
where
    Derivable: BonsaiDerivable,
{
    info!(ctx.logger(), "Warming {}", Derivable::NAME);
    let warmer: Box<WarmerFn> = Box::new({
        cloned!(repo_derived_data);
        move |ctx: &CoreContext, cs_id: ChangesetId| {
            cloned!(repo_derived_data);
            async move {
                repo_derived_data.derive::<Derivable>(ctx, cs_id).await?;
                Ok(())
            }
            .boxed()
        }
    });

    let is_warm: Box<IsWarmFn> = Box::new({
        move |ctx: &CoreContext, cs_id: ChangesetId| {
            let logger = ctx.logger().new(o!("type" => Derivable::NAME));
            cloned!(repo_derived_data);
            async move {
                let maybe_derived = repo_derived_data
                    .fetch_derived::<Derivable>(ctx, cs_id)
                    .await?;
                Ok(maybe_derived.is_some())
            }
            .watched(logger)
            .boxed()
        }
    });

    Warmer {
        warmer,
        is_warm,
        name: Derivable::NAME.to_string(),
    }
}

pub fn create_public_phase_warmer(ctx: &CoreContext, phases: ArcPhases) -> Warmer {
    info!(ctx.logger(), "Warming public phases");
    let warmer: Box<WarmerFn> = Box::new({
        cloned!(phases);
        move |ctx: &CoreContext, cs_id: ChangesetId| {
            cloned!(phases);
            async move {
                phases.add_reachable_as_public(ctx, vec![cs_id]).await?;
                Ok(())
            }
            .boxed()
        }
    });

    let is_warm: Box<IsWarmFn> = Box::new(move |ctx: &CoreContext, cs_id: ChangesetId| {
        cloned!(phases);
        async move {
            let maybe_public = phases
                .get_public(ctx, vec![cs_id], false /* ephemeral derive */)
                .await?;

            Ok(maybe_public.contains(&cs_id))
        }
        .boxed()
    });
    Warmer {
        warmer,
        is_warm,
        name: "public phases".to_string(),
    }
}
