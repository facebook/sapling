/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::builder::PossibleValuesParser;
use clap::Args;
use context::CoreContext;
use context::SessionClass;
use derived_data_utils::derived_data_utils;
use derived_data_utils::POSSIBLE_DERIVED_TYPES;
use futures_stats::TimedTryFutureExt;
use mononoke_app::args::ChangesetArgs;
use slog::trace;

use super::Repo;

#[derive(Args)]
pub(super) struct DeriveArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
    /// Type of derived data
    #[clap(long, short = 'T', value_parser = PossibleValuesParser::new(POSSIBLE_DERIVED_TYPES))]
    derived_data_type: String,
    /// Whether the changesets need to be rederived or not
    #[clap(long)]
    pub(crate) rederive: bool,
}

pub(super) async fn derive(ctx: &mut CoreContext, repo: &Repo, args: DeriveArgs) -> Result<()> {
    let derived_utils = derived_data_utils(ctx.fb, repo, args.derived_data_type)?;

    let csids = args.changeset_args.resolve_changesets(ctx, repo).await?;

    if args.rederive {
        trace!(ctx.logger(), "about to rederive {} commits", csids.len());
        derived_utils.regenerate(&csids);
        // Force this binary to write to all blobstores
        ctx.session_mut()
            .override_session_class(SessionClass::Background);
    } else {
        trace!(ctx.logger(), "about to derive {} commits", csids.len());
    };

    for csid in csids {
        trace!(ctx.logger(), "deriving {}", csid);
        let (stats, res) = derived_utils
            .derive(ctx.clone(), repo.repo_derived_data.clone(), csid)
            .try_timed()
            .await?;
        trace!(
            ctx.logger(),
            "derived {} in {}ms, {:?}",
            csid,
            stats.completion_time.as_millis(),
            res,
        );
    }

    Ok(())
}
