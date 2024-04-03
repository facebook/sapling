/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use bulk_derivation::BulkDerivation;
use clap::builder::PossibleValuesParser;
use clap::Args;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use derived_data_utils::POSSIBLE_DERIVED_TYPE_NAMES;
use futures::StreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use repo_derived_data::RepoDerivedDataRef;
use slog::trace;

use super::Repo;

#[derive(Args)]
pub(super) struct DeriveBulkArgs {
    /// Type of derived data
    #[clap(long, short = 'T', required = true,  value_parser = PossibleValuesParser::new(POSSIBLE_DERIVED_TYPE_NAMES), group="types to derive")]
    derived_data_types: Vec<String>,
    /// Whether all enabled derived data types should be derived
    #[clap(long, required = true, group = "types to derive")]
    all_types: bool,
    /// Commit ID of the start of the range.
    #[clap(long)]
    start: ChangesetId,
    /// Commit ID of the end of the range
    #[clap(long)]
    end: ChangesetId,
}

pub(super) async fn derive_bulk(
    ctx: &mut CoreContext,
    repo: &Repo,
    args: DeriveBulkArgs,
) -> Result<()> {
    let range_stream = repo
        .commit_graph()
        .range_stream(ctx, args.start, args.end)
        .await?;
    let csids = range_stream.collect::<Vec<_>>().await;
    if csids.is_empty() {
        return Err(anyhow!("no commits in range"));
    }

    let derived_data_types = if args.all_types {
        trace!(
            ctx.logger(),
            "active config types: {:?}",
            repo.repo_derived_data().active_config().types
        );
        // Derive all the types enabled in the config
        repo.repo_derived_data()
            .active_config()
            .types
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    } else {
        trace!(
            ctx.logger(),
            "user config types: {:?}",
            args.derived_data_types
        );
        // Only derive the types specified by the user
        args.derived_data_types
            .into_iter()
            .map(|ty| DerivableType::from_name(&ty))
            .collect::<Result<Vec<_>>>()?
    };

    trace!(
        ctx.logger(),
        "about to derive {} commits to {} types",
        csids.len(),
        derived_data_types.len()
    );
    repo.repo_derived_data()
        .manager()
        .derive_bulk(ctx, csids, None, &derived_data_types)
        .await?;

    Ok(())
}
