/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use clap::ValueEnum;
use cloned::cloned;
use commit_graph::AncestorsStreamBuilder;
use commit_graph::ArcCommitGraph;
use commit_graph::CommitGraphArc;
use context::CoreContext;
use context::SessionClass;
use derived_data_utils::DerivedUtils;
use futures::stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_stats::TimedTryFutureExt;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use slog::debug;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::task;

use super::args::DerivedUtilsArgs;
use super::slice::SliceDescription;
use super::Repo;

#[derive(Args)]
pub(super) struct DeriveSliceArgs {
    /// File containing a JSON array of slices.
    #[clap(long, short = 'f', value_name = "FILE")]
    slice_description_file: PathBuf,

    #[clap(flatten)]
    derived_utils_args: DerivedUtilsArgs,

    /// Whether to derive only the heads of each slice or all commits excluding the heads.
    #[clap(long)]
    mode: DeriveSliceMode,

    /// Maximum number of slices to process concurrently.
    #[clap(long, default_value_t = 10)]
    slice_concurrency: usize,

    /// Maximum number of heads to derive concurrently.
    #[clap(long, default_value_t = 10)]
    heads_concurrency: usize,

    /// Whether or not to rederive changesets that are already derived.
    #[clap(long)]
    pub(crate) rederive: bool,
}

#[derive(Clone, ValueEnum)]
enum DeriveSliceMode {
    /// Derive only the heads of each slice using other derived data types.
    HeadsOnly,
    /// Derive all commits in the slice excluding the heads.
    ExcludingHeads,
}

async fn parse_slice_descriptions(
    slice_description_file: PathBuf,
) -> Result<Vec<SliceDescription>> {
    let mut file = File::open(&slice_description_file)
        .await
        .context("Failed to open slice description file")?;

    let mut contents = vec![];
    file.read_to_end(&mut contents)
        .await
        .context("Failed to read slice description file")?;

    serde_json::from_slice(&contents).context("Failed to parse slice description")
}

async fn derive_slice_heads_only(
    ctx: &CoreContext,
    repo_derived_data: ArcRepoDerivedData,
    derived_utils: Arc<dyn DerivedUtils>,
    slice_description: SliceDescription,
    heads_concurrency: usize,
    rederive: bool,
) -> Result<()> {
    let mut ctx = ctx.clone();
    if rederive {
        derived_utils.regenerate(&slice_description.slice_frontier);
        // Force this binary to write to all blobstores
        ctx.session_mut()
            .override_session_class(SessionClass::Background);
    }

    let derivation_results = stream::iter(slice_description.slice_frontier)
        .map(|csid| {
            cloned!(ctx, derived_utils, repo_derived_data);
            async move {
                task::spawn(async move {
                    derived_utils
                        .derive_from_predecessor(ctx, repo_derived_data, csid)
                        .try_timed()
                        .map_ok(move |res| (csid, res))
                        .await
                })
                .await?
            }
        })
        .buffered(heads_concurrency)
        .try_collect::<Vec<_>>()
        .await?;

    for (csid, (stats, res)) in derivation_results {
        debug!(
            ctx.logger(),
            "derived {} in {}ms, {:?}",
            csid,
            stats.completion_time.as_millis(),
            res,
        );
    }

    Ok(())
}

/// Returns all changesets that are part of the given slice i.e. that
/// are ancestors of `slice_description.slice_frontier` and have generation
/// greater than or equal to `slice_description.slice_start`.
async fn find_all_slice_changesets(
    ctx: &CoreContext,
    commit_graph: ArcCommitGraph,
    slice_description: &SliceDescription,
) -> Result<Vec<ChangesetId>> {
    let slice_start = slice_description.slice_start;
    AncestorsStreamBuilder::new(
        commit_graph.clone(),
        ctx.clone(),
        slice_description.slice_frontier.clone(),
    )
    .with({
        cloned!(ctx);
        move |cs_id| {
            cloned!(ctx, commit_graph);
            async move {
                let generation = commit_graph.changeset_generation(&ctx, cs_id).await?;
                Ok(generation >= slice_start)
            }
        }
    })
    .build()
    .await?
    .try_collect::<Vec<_>>()
    .await
}

/// Returns all underived changesets that are part of the given slice i.e. that
/// are ancestors of `slice_description.slice_frontier` and have generation
/// greater than or equal to `slice_description.slice_start`.
///
/// Note 1: All underived changesets will be returned, but it is possible to
/// additionally return derived changesets.
///
/// Note 2: This assumes that the derived changesets in the slice aside from
/// heads are derived without gaps (i.e. all their ancestors in the slice
/// are derived). If there are any gaps the --rederive flag should be used.
async fn find_underived_slice_changesets(
    ctx: &CoreContext,
    commit_graph: ArcCommitGraph,
    derived_utils: Arc<dyn DerivedUtils>,
    slice_description: &SliceDescription,
) -> Result<Vec<ChangesetId>> {
    // Find the minimum generation out of all changesets in the slice frontier.
    // This is used to determine when to stop traversing ancestors, by stopping
    // at changesets that have derived data and also have a lower generation than
    // the mininum slice frontier generation. This is needed because the slice
    // frontier might be derived before the rest of slice:
    //
    //                 E  -> slice frontier, derived
    //                /|
    //    derived <- F D  -> slice frontier, derived
    //               | |
    //    derived <- G C  -> underived
    //               | |
    //    derived <- H B  -> underived
    //               | |
    //    derived <- I A  -> derived
    //
    // For this example, we would traverse [E, F, D, G, C, B]. Ideally we would also stop at F
    // before traversing to G, but it seems to be complicated to implement without much benefit.
    let min_slice_frontier_generation = stream::iter(slice_description.slice_frontier.clone())
        .map(|cs_id| commit_graph.changeset_generation(ctx, cs_id))
        .buffered(100)
        .try_fold(
            Generation::new(u64::MAX),
            |min_generation, current_generation| async move {
                Ok(std::cmp::min(min_generation, current_generation))
            },
        )
        .await?;

    let slice_start = slice_description.slice_start;
    AncestorsStreamBuilder::new(
        commit_graph.clone(),
        ctx.clone(),
        slice_description.slice_frontier.clone(),
    )
    .with({
        cloned!(ctx, derived_utils);
        move |cs_id| {
            cloned!(ctx, commit_graph, derived_utils);
            async move {
                let generation = commit_graph.changeset_generation(&ctx, cs_id).await?;

                if generation < slice_start {
                    return Ok(false);
                }

                if generation < min_slice_frontier_generation
                    && derived_utils.is_derived(&ctx, cs_id).await?
                {
                    return Ok(false);
                }

                Ok(true)
            }
        }
    })
    .build()
    .await?
    .try_collect::<Vec<_>>()
    .await
}

async fn derive_slice_excluding_heads(
    ctx: &CoreContext,
    commit_graph: ArcCommitGraph,
    repo_derived_data: ArcRepoDerivedData,
    derived_utils: Arc<dyn DerivedUtils>,
    slice_description: SliceDescription,
    rederive: bool,
) -> Result<()> {
    let ancestors = if rederive {
        find_all_slice_changesets(ctx, commit_graph.clone(), &slice_description).await?
    } else {
        find_underived_slice_changesets(
            ctx,
            commit_graph.clone(),
            derived_utils.clone(),
            &slice_description,
        )
        .await?
    };

    // Exclude the frontier of the slice from the list of ancestors.
    let heads = slice_description
        .slice_frontier
        .into_iter()
        .collect::<HashSet<_>>();
    let exclusive_ancestors = ancestors
        .into_iter()
        .filter(|ancestor| !heads.contains(ancestor))
        .collect::<Vec<_>>();

    let mut ctx = ctx.clone();
    if rederive {
        derived_utils.regenerate(&exclusive_ancestors);
        // Force this binary to write to all blobstores
        ctx.session_mut()
            .override_session_class(SessionClass::Background);
    }

    let (stats, ()) = commit_graph
        .process_topologically(&ctx, exclusive_ancestors, |ancestor| {
            derived_utils
                .derive_exactly_batch(
                    ctx.clone(),
                    repo_derived_data.clone(),
                    vec![ancestor],
                    false,
                    None,
                )
                .map_ok(|_| ())
        })
        .try_timed()
        .await?;

    debug!(
        ctx.logger(),
        "derived exclusive ancestors in {}ms",
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
    let slice_descriptions = parse_slice_descriptions(args.slice_description_file).await?;

    match args.mode {
        DeriveSliceMode::HeadsOnly => {
            stream::iter(slice_descriptions)
                .map(Ok)
                .try_for_each_concurrent(args.slice_concurrency, |slice_description| {
                    cloned!(ctx, derived_utils);
                    let repo_derived_data = repo.repo_derived_data_arc();
                    async move {
                        task::spawn(async move {
                            derive_slice_heads_only(
                                &ctx,
                                repo_derived_data,
                                derived_utils,
                                slice_description,
                                args.heads_concurrency,
                                args.rederive,
                            )
                            .await
                        })
                        .await?
                    }
                })
                .await
        }
        DeriveSliceMode::ExcludingHeads => {
            stream::iter(slice_descriptions)
                .map(Ok)
                .try_for_each_concurrent(args.slice_concurrency, |slice_description| {
                    cloned!(ctx, derived_utils);
                    let commit_graph = repo.commit_graph_arc();
                    let repo_derived_data = repo.repo_derived_data_arc();
                    async move {
                        task::spawn(async move {
                            derive_slice_excluding_heads(
                                &ctx,
                                commit_graph,
                                repo_derived_data,
                                derived_utils,
                                slice_description,
                                args.rederive,
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
