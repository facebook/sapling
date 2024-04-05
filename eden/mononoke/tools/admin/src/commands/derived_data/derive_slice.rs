/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use clap::ValueEnum;
use cloned::cloned;
use commit_graph::ArcCommitGraph;
use commit_graph::CommitGraphArc;
use commit_graph_types::segments::BoundaryChangesets;
use commit_graph_types::segments::SegmentedSliceDescription;
use context::CoreContext;
use context::SessionClass;
use derived_data_utils::DerivedUtils;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedTryFutureExt;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use slog::debug;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::task;

use super::args::DerivedUtilsArgs;
use super::Repo;

#[derive(Args)]
pub(super) struct DeriveSliceArgs {
    /// JSON file containing either deserialized `Vec<SegmentedSliceDescription>` or `BoundaryChangesets`
    /// dependeing on `mode` arg.
    #[clap(long, short = 'f', value_name = "FILE")]
    input_file: PathBuf,

    #[clap(flatten)]
    derived_utils_args: DerivedUtilsArgs,

    /// Whether to derive slices or the boundaries between slices.
    #[clap(long)]
    mode: DeriveSliceMode,

    /// Maximum number of slices to process concurrently.
    #[clap(long, default_value_t = 10)]
    slice_concurrency: usize,

    /// Maximum number of boundaries to derive concurrently.
    #[clap(long, default_value_t = 10)]
    boundaries_concurrency: usize,

    /// Whether or not to rederive changesets that are already derived.
    #[clap(long)]
    pub(crate) rederive: bool,
}

#[derive(Clone, ValueEnum)]
enum DeriveSliceMode {
    Boundaries,
    Slices,
}

async fn parse_slice_descriptions(
    slice_descriptions_file: PathBuf,
) -> Result<Vec<SegmentedSliceDescription>> {
    let mut file = File::open(&slice_descriptions_file)
        .await
        .context("Failed to open slice descriptions file")?;

    let mut contents = vec![];
    file.read_to_end(&mut contents)
        .await
        .context("Failed to read slice descriptions file")?;

    serde_json::from_slice(&contents).context("Failed to parse slice descriptions")
}

async fn parse_boundaries(boundaries_files: PathBuf) -> Result<BoundaryChangesets> {
    let mut file = File::open(&boundaries_files)
        .await
        .context("Failed to open boundaries file")?;

    let mut contents = vec![];
    file.read_to_end(&mut contents)
        .await
        .context("Failed to read boundaries file")?;

    serde_json::from_slice(&contents).context("Failed to parse boundaries")
}

async fn derive_boundaries(
    ctx: &CoreContext,
    repo_derived_data: ArcRepoDerivedData,
    derived_utils: Arc<dyn DerivedUtils>,
    boundaries: BoundaryChangesets,
    boundaries_concurrency: usize,
) -> Result<()> {
    stream::iter(boundaries)
        .map(Ok)
        .try_for_each_concurrent(boundaries_concurrency, |csid| {
            cloned!(ctx, derived_utils, repo_derived_data);
            async move {
                task::spawn(async move {
                    let (derive_boundary_stats, res) = derived_utils
                        .derive_from_predecessor(ctx.clone(), repo_derived_data, csid)
                        .try_timed()
                        .await?;

                    debug!(
                        ctx.logger(),
                        "derived boundary {} in {}ms, {:?}",
                        csid,
                        derive_boundary_stats.completion_time.as_millis(),
                        res,
                    );

                    Ok(())
                })
                .await?
            }
        })
        .await
}

async fn inner_derive_slice(
    ctx: &CoreContext,
    commit_graph: ArcCommitGraph,
    repo_derived_data: ArcRepoDerivedData,
    derived_utils: Arc<dyn DerivedUtils>,
    slice_description: SegmentedSliceDescription,
) -> Result<()> {
    let (stats, ()) = stream::iter(slice_description.segments)
        .map(anyhow::Ok)
        .try_for_each(|segment| {
            cloned!(ctx, commit_graph, derived_utils, repo_derived_data);
            async move {
                let segment_cs_ids = commit_graph
                    .range_stream(&ctx, segment.base, segment.head)
                    .await?
                    .collect::<Vec<_>>()
                    .await;

                let (derive_segment_stats, _) = derived_utils
                    .derive_exactly_batch(ctx.clone(), repo_derived_data.clone(), segment_cs_ids)
                    .try_timed()
                    .await?;

                debug!(
                    ctx.logger(),
                    "derived segment from {} to {} in {}ms",
                    segment.base,
                    segment.head,
                    derive_segment_stats.completion_time.as_millis(),
                );

                Ok(())
            }
        })
        .try_timed()
        .await?;

    debug!(
        ctx.logger(),
        "derived slice in {}ms",
        stats.completion_time.as_millis(),
    );

    Ok(())
}

pub(super) async fn derive_slice(
    ctx: &CoreContext,
    repo: &Repo,
    args: DeriveSliceArgs,
) -> Result<()> {
    let derived_utils = args.derived_utils_args.derived_utils(ctx, repo)?;
    if args.rederive {
        let mut ctx = ctx.clone();
        // Force this binary to write to all blobstores
        ctx.session_mut()
            .override_session_class(SessionClass::Background);
    }

    match args.mode {
        DeriveSliceMode::Boundaries => {
            let boundaries = parse_boundaries(args.input_file).await?;

            derive_boundaries(
                ctx,
                repo.repo_derived_data_arc(),
                derived_utils,
                boundaries,
                args.boundaries_concurrency,
            )
            .await
        }
        DeriveSliceMode::Slices => {
            let slice_descriptions = parse_slice_descriptions(args.input_file).await?;

            stream::iter(slice_descriptions)
                .map(Ok)
                .try_for_each_concurrent(args.slice_concurrency, |slice_description| {
                    cloned!(ctx, derived_utils);
                    let commit_graph = repo.commit_graph_arc();
                    let repo_derived_data = repo.repo_derived_data_arc();
                    async move {
                        task::spawn(async move {
                            inner_derive_slice(
                                &ctx,
                                commit_graph,
                                repo_derived_data,
                                derived_utils,
                                slice_description,
                            )
                            .await
                        })
                        .await?
                    }
                })
                .await
        }
    }
}
