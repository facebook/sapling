/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bulk_derivation::BulkDerivation;
use clap::builder::PossibleValuesParser;
use clap::Args;
use clap::ValueEnum;
use cloned::cloned;
use commit_graph::ArcCommitGraph;
use commit_graph::CommitGraphArc;
use commit_graph_types::segments::BoundaryChangesets;
use commit_graph_types::segments::SegmentedSliceDescription;
use context::CoreContext;
use context::SessionClass;
use derived_data_manager::DerivedDataManager;
use futures::stream;
use futures::try_join;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedTryFutureExt;
use mononoke_types::DerivableType;
use repo_derived_data::RepoDerivedDataRef;
use slog::debug;
use strum::IntoEnumIterator;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::task;

use super::Repo;

#[derive(Args)]
pub(super) struct DeriveSliceArgs {
    /// JSON file containing either deserialized `Vec<SegmentedSliceDescription>` or `BoundaryChangesets`
    /// dependeing on `mode` arg.
    #[clap(long, short = 'f', value_name = "FILE")]
    input_file: PathBuf,

    #[clap(short = 'T', long, value_parser = PossibleValuesParser::new(DerivableType::iter().map(|t| DerivableType::name(&t))))]
    derived_data_type: String,

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
    manager: DerivedDataManager,
    boundaries: BoundaryChangesets,
    boundaries_concurrency: usize,
    derived_data_type: DerivableType,
) -> Result<()> {
    let boundaries_count = boundaries.len();
    debug!(
        ctx.logger(),
        "deriving {} boundaries (concurrency: {})", boundaries_count, boundaries_concurrency
    );
    let completed = Arc::new(AtomicUsize::new(0));
    stream::iter(boundaries)
        .map(Ok)
        .try_for_each_concurrent(boundaries_concurrency, |csid| {
            cloned!(ctx, manager, completed);
            async move {
                task::spawn(async move {
                    let (derive_boundary_stats, ()) = BulkDerivation::derive_from_predecessor(
                        &manager,
                        &ctx,
                        csid,
                        None,
                        derived_data_type,
                    )
                    .try_timed()
                    .await?;

                    let completed_count = completed.fetch_add(1, Ordering::SeqCst) + 1;
                    debug!(
                        ctx.logger(),
                        "derived boundary {} in {}ms, ({}/{})",
                        csid,
                        derive_boundary_stats.completion_time.as_millis(),
                        completed_count,
                        boundaries_count,
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
    manager: DerivedDataManager,
    slice_description: SegmentedSliceDescription,
    slice_count: usize,
    completed: Arc<AtomicUsize>,
    derived_data_type: DerivableType,
) -> Result<()> {
    let segment_count = slice_description.segments.len();
    let (stats, ()) = stream::iter(slice_description.segments.into_iter().enumerate())
        .map(anyhow::Ok)
        .try_for_each(|(segment_index, segment)| {
            cloned!(ctx, commit_graph, manager);
            async move {
                let (head_generation, base_generation) = try_join!(
                    commit_graph.changeset_generation(&ctx, segment.head),
                    commit_graph.changeset_generation(&ctx, segment.base)
                )?;
                let segment_cs_ids_count = head_generation.value() - base_generation.value() + 1;

                debug!(
                    ctx.logger(),
                    "deriving segment from {} to {} ({} commits, {}/{})",
                    segment.base,
                    segment.head,
                    segment_cs_ids_count,
                    segment_index + 1,
                    segment_count,
                );

                let (derive_batch_stats, ()) = manager
                    .derive_bulk(&ctx, &[segment.head], None, &[derived_data_type], None)
                    .try_timed()
                    .await?;

                debug!(
                    ctx.logger(),
                    "derived segment from {} to {} in {}ms ({}/{})",
                    segment.base,
                    segment.head,
                    derive_batch_stats.completion_time.as_millis(),
                    segment_index + 1,
                    segment_count,
                );

                Ok(())
            }
        })
        .try_timed()
        .await?;

    let completed_count = completed.fetch_add(1, Ordering::SeqCst) + 1;
    debug!(
        ctx.logger(),
        "derived slice in {}ms ({}/{})",
        stats.completion_time.as_millis(),
        completed_count,
        slice_count,
    );

    Ok(())
}

pub(super) async fn derive_slice(
    ctx: &CoreContext,
    repo: &Repo,
    args: DeriveSliceArgs,
) -> Result<()> {
    let derived_data_type = DerivableType::from_name(&args.derived_data_type)?;
    let manager = repo.repo_derived_data().manager().clone();

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
                manager,
                boundaries,
                args.boundaries_concurrency,
                derived_data_type,
            )
            .await
        }
        DeriveSliceMode::Slices => {
            let slice_descriptions = parse_slice_descriptions(args.input_file).await?;
            let slice_count = slice_descriptions.len();
            let completed = Arc::new(AtomicUsize::new(0));
            debug!(
                ctx.logger(),
                "deriving {} slices (concurrency: {})", slice_count, args.slice_concurrency
            );

            stream::iter(slice_descriptions)
                .map(Ok)
                .try_for_each_concurrent(args.slice_concurrency, |slice_description| {
                    cloned!(ctx, manager, completed);
                    let commit_graph = repo.commit_graph_arc();
                    async move {
                        task::spawn(async move {
                            inner_derive_slice(
                                &ctx,
                                commit_graph,
                                manager,
                                slice_description,
                                slice_count,
                                completed,
                                derived_data_type,
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
