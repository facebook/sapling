/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use futures_stats::TimedTryFutureExt;
use mononoke_app::args::ChangesetArgs;
use phases::PhasesRef;
use slog::info;

use super::Repo;

#[derive(Args)]
pub(super) struct AddPublicArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
}

pub(super) async fn add_public(ctx: &CoreContext, repo: &Repo, args: AddPublicArgs) -> Result<()> {
    let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;

    info!(
        ctx.logger(),
        "Marking ancestors of {} commits as public",
        cs_ids.len()
    );

    let (stats, _) = repo
        .phases()
        .add_reachable_as_public(ctx, cs_ids)
        .try_timed()
        .await?;

    info!(
        ctx.logger(),
        "Finished marking ancestors as public in {:?}", stats
    );

    Ok(())
}
