/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use anyhow::anyhow;
use bulk_derivation::BulkDerivation;
use clap::Args;
use commit_graph::CommitGraphArc;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use derived_data_manager::DerivedDataManager;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;
use mononoke_types::ChangesetId;

use super::Repo;

#[derive(Args)]
pub(super) struct FindDerivationGapsArgs {
    /// Head(s) to walk back from (accepts -B/--bookmark, e.g. -B master).
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,

    /// Sample every Nth ancestor, measured in generation numbers (the slice size).
    #[clap(long, default_value_t = 1000)]
    step: u64,

    /// Process history in windows of this many generations at a time. Bounds
    /// per-iteration work and emits a progress line per window.
    #[clap(long, default_value_t = 1_000_000)]
    batch_generations: u64,

    /// Stop sampling once we reach this generation (inclusive floor).
    #[clap(long, default_value_t = 1)]
    min_generation: u64,

    /// Maximum number of concurrent derivation checks.
    #[clap(long, default_value_t = 100)]
    concurrency: usize,

    /// Emit a progress line after traversing this many generations with no gap.
    #[clap(long, default_value_t = 1_000_000)]
    report_interval: u64,
}

pub(super) async fn find_derivation_gaps(
    ctx: &CoreContext,
    repo: &Repo,
    manager: &DerivedDataManager,
    args: FindDerivationGapsArgs,
) -> Result<()> {
    let derived_data_type = args.derived_data_args.resolve_type()?;
    let heads = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let commit_graph = repo.commit_graph();

    if args.step == 0 {
        return Err(anyhow!("--step must be greater than 0"));
    }
    if args.batch_generations == 0 {
        return Err(anyhow!("--batch-generations must be greater than 0"));
    }

    let top = stream::iter(heads.clone())
        .map(|cs_id| commit_graph.changeset_generation(ctx, cs_id))
        .buffered(10)
        .try_fold(0u64, |acc, generation| async move {
            Ok(acc.max(generation.value()))
        })
        .await?;

    eprintln!(
        "scanning {} for derivation gaps from generation {} down to {} (step {})",
        derived_data_type, top, args.min_generation, args.step,
    );

    // State carried across windows.
    let mut current_heads = heads;
    let mut window_top = top;
    // The lowest band of one window overlaps the top band of the next (we resume
    // from it); skip those commits to avoid double-checking.
    let mut prev_resume: HashSet<ChangesetId> = HashSet::new();
    // When inside a measured gap, skip (without checking) every sampled boundary
    // whose generation is above this watermark -- we already know it's underived.
    let mut skip_above_gen: Option<u64> = None;
    // Top generation of the current uninterrupted run with no gaps, for progress.
    let mut clean_since: Option<u64> = None;

    let mut sampled: u64 = 0;
    let mut gaps: u64 = 0;
    let mut underived_total: u64 = 0;

    while window_top > args.min_generation {
        let batch_floor = window_top
            .saturating_sub(args.batch_generations)
            .max(args.min_generation);

        // Sample this window: slice [batch_floor, window_top] into bands of `step`
        // and take each band's frontier commit. slice_ancestors labels each slice by
        // the band floor, but the commits are the frontier commits whose *true*
        // generation lies in the band, so we fetch real generations below.
        let slices = commit_graph
            .slice_ancestors(
                ctx,
                current_heads.clone(),
                |cs_ids| async move {
                    stream::iter(cs_ids)
                        .map(|cs_id| async move {
                            anyhow::Ok((
                                cs_id,
                                repo.commit_graph_arc()
                                    .changeset_generation(ctx, cs_id)
                                    .await?,
                            ))
                        })
                        .buffered(10)
                        .try_filter_map(|(cs_id, generation)| async move {
                            Ok((generation.value() >= batch_floor).then_some(cs_id))
                        })
                        .try_collect()
                        .await
                },
                args.step,
            )
            .await?;

        if slices.is_empty() {
            break;
        }

        let sampled_cs_ids: Vec<ChangesetId> = slices
            .iter()
            .flat_map(|(_label, cs_ids)| cs_ids.iter().copied())
            .filter(|cs_id| !prev_resume.contains(cs_id))
            .collect();

        let mut boundaries: Vec<(u64, ChangesetId)> = stream::iter(sampled_cs_ids)
            .map(|cs_id| async move {
                anyhow::Ok((
                    commit_graph.changeset_generation(ctx, cs_id).await?.value(),
                    cs_id,
                ))
            })
            .buffered(args.concurrency.max(1))
            .try_collect()
            .await?;
        boundaries.sort_by(|a, b| b.0.cmp(&a.0));

        // Check derivation in concurrency-sized chunks, then scan results in
        // generation order to detect, size, and skip gaps.
        for chunk in boundaries.chunks(args.concurrency.max(1)) {
            let to_check: Vec<(u64, ChangesetId)> = chunk
                .iter()
                .copied()
                .filter(|(generation, _)| match skip_above_gen {
                    Some(floor) => *generation <= floor,
                    None => true,
                })
                .collect();
            if to_check.is_empty() {
                continue;
            }

            let checked: Vec<(u64, ChangesetId, bool)> = stream::iter(to_check)
                .map(|(generation, cs_id)| async move {
                    let derived = manager
                        .is_derived(ctx, cs_id, None, derived_data_type)
                        .await?;
                    anyhow::Ok((generation, cs_id, derived))
                })
                .buffered(args.concurrency.max(1))
                .try_collect()
                .await?;

            for (generation, cs_id, derived) in checked {
                if let Some(floor) = skip_above_gen {
                    if generation > floor {
                        continue;
                    }
                    skip_above_gen = None;
                }

                sampled += 1;

                if derived {
                    let since = *clean_since.get_or_insert(generation);
                    if since.saturating_sub(generation) >= args.report_interval {
                        println!("CLEAN no gaps from generation {since} to {generation}");
                        clean_since = Some(generation);
                    }
                    continue;
                }

                // Underived boundary: the highest sampled commit of a gap. Reuse
                // count-underived's frontier logic to size the gap and find where
                // derived history resumes, then skip the rest of the gap.
                clean_since = None;

                let frontier = commit_graph
                    .ancestors_frontier_with(ctx, vec![cs_id], |candidate| async move {
                        Ok(manager
                            .is_derived(ctx, candidate, None, derived_data_type)
                            .await?)
                    })
                    .await?;

                let gap_size: u64 = commit_graph
                    .ancestors_difference_segments(ctx, vec![cs_id], frontier.clone())
                    .await?
                    .into_iter()
                    .map(|segment| segment.length)
                    .sum();

                let frontier_gen = stream::iter(frontier)
                    .map(|c| commit_graph.changeset_generation(ctx, c))
                    .buffered(10)
                    .try_fold(0u64, |acc, generation| async move {
                        Ok(acc.max(generation.value()))
                    })
                    .await?;

                gaps += 1;
                underived_total += gap_size;
                println!("GAP generation={generation} size={gap_size} {cs_id}");
                skip_above_gen = Some(frontier_gen);
            }
        }

        // Resume from the lowest band of this window.
        let (low_label, low_heads) = match slices.into_iter().min_by_key(|(label, _)| label.value())
        {
            Some(lowest) => lowest,
            None => break,
        };
        let low_label = low_label.value();

        eprintln!(
            "progress: scanned generation {window_top} down to {low_label} \
             ({sampled} checked, {gaps} gap(s), {underived_total} underived so far)"
        );

        if low_label <= args.min_generation || low_label >= window_top {
            break;
        }
        prev_resume = low_heads.iter().copied().collect();
        current_heads = low_heads;
        window_top = low_label;
    }

    if let Some(since) = clean_since {
        if since > args.min_generation {
            println!(
                "CLEAN no gaps from generation {since} to {}",
                args.min_generation
            );
        }
    }

    eprintln!(
        "done: checked {sampled} boundary commits, found {gaps} gap(s) \
         totalling {underived_total} underived commit(s)"
    );
    Ok(())
}
