/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use bulk_derivation::BulkDerivation;
use clap::Args;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use derived_data_manager::DerivedDataManager;
use futures_stats::TimedTryFutureExt;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;
use slog::debug;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use super::Repo;

#[derive(Args)]
pub(super) struct SliceArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,

    /// The size of each slice in generation numbers. So that each slice will have
    /// changesets with generations in a range [slice_start, slice_start + slice_size)
    #[clap(long)]
    slice_size: u64,

    /// If set, will slice all ancestors of the given commits. Regardless of whether
    /// they are already derived or not.
    #[clap(long)]
    reslice: bool,

    /// If provided, the output slices will be written to this file as a JSON array.
    /// Otherwise, they will be printed to stdout.
    #[clap(long, short = 'o', value_name = "FILE")]
    output_json_file: Option<PathBuf>,
}

pub(super) async fn slice(
    ctx: &CoreContext,
    repo: &Repo,
    manager: &DerivedDataManager,
    args: SliceArgs,
) -> Result<()> {
    let mut cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let derived_data_type = args.derived_data_args.resolve_type()?;

    debug!(
        ctx.logger(),
        "slicing ancestors of {} changesets",
        cs_ids.len(),
    );

    let excluded_ancestors = if args.reslice {
        vec![]
    } else {
        cs_ids = manager
            .pending(ctx, &cs_ids, None, derived_data_type)
            .await?;

        let (frontier_stats, frontier) = repo
            .commit_graph()
            .ancestors_frontier_with(ctx, cs_ids.clone(), |cs_id| async move {
                Ok(manager
                    .is_derived(ctx, cs_id, None, derived_data_type)
                    .await?)
            })
            .try_timed()
            .await?;
        debug!(
            ctx.logger(),
            "calculated derived frontier ({} changesets) in {}ms",
            frontier.len(),
            frontier_stats.completion_time.as_millis(),
        );
        frontier
    };

    let (slices_stats, (slices, boundary_changesets)) = repo
        .commit_graph()
        .segmented_slice_ancestors(ctx, cs_ids, excluded_ancestors, args.slice_size)
        .try_timed()
        .await?;
    debug!(
        ctx.logger(),
        "calculated slices in {}ms",
        slices_stats.completion_time.as_millis(),
    );

    if let Some(output_json_file) = args.output_json_file {
        let mut file = File::create(output_json_file)
            .await
            .context("Failed to create output file")?;
        file.write_all(serde_json::to_string(&(slices, boundary_changesets))?.as_bytes())
            .await
            .context("Failed to write slices to output file")?;
        file.flush().await?;
    } else {
        println!("Slices:");
        for slice in slices {
            println!(
                "{}",
                slice
                    .segments
                    .into_iter()
                    .map(|segment| format!("{}->{}", segment.head, segment.base))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }

        println!("Boundary changesets:");
        for cs_id in boundary_changesets {
            println!("{}", cs_id);
        }
    }

    Ok(())
}
