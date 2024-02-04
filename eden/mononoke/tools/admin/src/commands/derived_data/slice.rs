/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::future;
use futures::TryFutureExt;
use mononoke_app::args::ChangesetArgs;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use repo_derived_data::RepoDerivedDataArc;
use serde::Deserialize;
use serde::Serialize;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use super::args::DerivedUtilsArgs;
use super::Repo;

/// A description of a slice of the commit graph. All commits that are ancestors of
/// `slice_frontier` and have generation numbers higher than or equal to `slice_start`
/// are considered part of the slice.
#[derive(Serialize, Deserialize)]
pub(super) struct SliceDescription {
    pub slice_frontier: Vec<ChangesetId>,
    pub slice_start: Generation,
}

#[derive(Args)]
pub(super) struct SliceArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_utils_args: DerivedUtilsArgs,

    /// The size of each slice in generation numbers. So that each slice will have
    /// changesets with generations in a range [slice_start, slice_start + slice_size)
    #[clap(long)]
    slice_size: u64,

    /// If set, will slice all ancestors of the given commits. Regardless of whether
    /// they are already derived or not.
    #[clap(long)]
    rederive: bool,

    /// If provided, the output slices will be written to this file as a JSON array.
    /// Otherwise, they will be printed to stdout.
    #[clap(long, short = 'o', value_name = "FILE")]
    output_json_file: Option<PathBuf>,
}

pub(super) async fn slice(ctx: &CoreContext, repo: &Repo, args: SliceArgs) -> Result<()> {
    let derived_utils = args.derived_utils_args.derived_utils(ctx, repo)?;

    let csids = args.changeset_args.resolve_changesets(ctx, repo).await?;

    let slices = if args.rederive {
        repo.commit_graph()
            .slice_ancestors(
                ctx,
                csids,
                |csids| future::ok(csids.into_iter().collect()),
                args.slice_size,
            )
            .await?
    } else {
        repo.commit_graph()
            .slice_ancestors(
                ctx,
                csids,
                |csids| {
                    derived_utils
                        .pending(ctx.clone(), repo.repo_derived_data_arc(), csids)
                        .map_ok(|pending_csids| pending_csids.into_iter().collect())
                },
                args.slice_size,
            )
            .await?
    };

    if let Some(output_json_file) = args.output_json_file {
        let mut file = File::create(output_json_file)
            .await
            .context("Failed to create output file")?;
        file.write_all(
            serde_json::to_string(
                &slices
                    .into_iter()
                    .map(|(slice_start, slice_frontier)| SliceDescription {
                        slice_frontier,
                        slice_start,
                    })
                    .collect::<Vec<_>>(),
            )?
            .as_bytes(),
        )
        .await
        .context("Failed to write slices to output file")?;
        file.flush().await?;
    } else {
        for (slice_start, slice_frontier) in slices {
            let slice_frontier = slice_frontier
                .into_iter()
                .map(|cs_id| format!("{}", cs_id))
                .collect::<Vec<_>>();
            println!(
                "[{}, {}): {}",
                slice_start.value(),
                slice_start.value() + args.slice_size,
                slice_frontier.join(" ")
            );
        }
    }

    Ok(())
}
