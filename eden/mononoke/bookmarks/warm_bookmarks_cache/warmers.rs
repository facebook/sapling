/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::IsWarmFn;
use super::Warmer;
use super::WarmerFn;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use futures::future::FutureExt;
use futures_watchdog::WatchdogExt;
use mononoke_types::ChangesetId;
use phases::PhasesArc;
use repo_derived_data::RepoDerivedDataArc;
use slog::info;
use slog::o;

pub fn create_derived_data_warmer<Derivable, Repo>(ctx: &CoreContext, repo: &Repo) -> Warmer
where
    Derivable: BonsaiDerivable,
    Repo: RepoDerivedDataArc,
{
    info!(ctx.logger(), "Warming {}", Derivable::NAME);
    let repo_derived_data = repo.repo_derived_data_arc();

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

pub fn create_public_phase_warmer(ctx: &CoreContext, repo: &impl PhasesArc) -> Warmer {
    info!(ctx.logger(), "Warming public phases");
    let phases = repo.phases_arc();
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
