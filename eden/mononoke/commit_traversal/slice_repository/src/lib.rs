/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Error;
use anyhow::Result;
use changeset_fetcher::ChangesetFetcherArc;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Future;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use skiplist::SkiplistIndex;
use slog::info;

pub trait Repo = ChangesetFetcherArc;

/// If skiplist parents are not available, fetch the parents and their
/// generation from the repo.
async fn parents_with_generations(
    ctx: &CoreContext,
    repo: &impl Repo,
    csid: ChangesetId,
) -> Result<Vec<(ChangesetId, Generation)>> {
    let parents = repo.changeset_fetcher().get_parents(ctx, csid).await?;
    let parents_with_generations =
        stream::iter(parents.into_iter().map(|parent_csid| async move {
            let gen = repo
                .changeset_fetcher()
                .get_generation_number(ctx, parent_csid)
                .await?;
            Ok(Some((parent_csid, gen)))
        }))
        .buffered(100)
        .try_filter_map(|maybe_csid_gen| async move { Ok::<_, Error>(maybe_csid_gen) })
        .try_collect::<Vec<_>>()
        .await?;
    Ok(parents_with_generations)
}

/// Slice a respository into a sequence of slices for processing.
///
/// Notice that "processing" can be any operation that needs to be
/// done on commits that needs to be done first on its ancestors.
/// The `needs_processing` function should return which commits still
/// need processing.
///
/// For large repositories with a long history, computing the full set of
/// commits before beginning backfilling is slow, and cannot be resumed
/// if interrupted.
///
/// This function makes large repositories more tractible by using the
/// skiplist index to divide the repository history into "slices", where
/// each slice consists of the commits known to the skiplist index that
/// are within a range of generations.
///
/// Each slice's heads should be derived together and will be ancestors of
/// subsequent slices.  The returned slices consist only of heads which
/// haven't been derived by the provided needs_processing function. Slicing
/// stops once all derived commits are reached.
///
/// For example, given a repository where the skiplists have the structure:
///
/// ```text
///     E (gen 450)
///     :
///     D (gen 350)
///     :
///     : C (gen 275)
///     :/
///     B (gen 180)
///     :
///     A (gen 1)
/// ```
///
/// And a slice size of 200, this function will generate slices:
///
/// ```text
///     (0, [A, B])
///     (200, [C, D])
///     (400, [E])
/// ```
///
/// If any of these heads are already derived then they are omitted.  Empty
/// slices are also omitted.
///
/// This allows derivation of the first slice with underived commits to begin
/// more quickly, as the rest of the repository history doesn't need to be
/// traversed (just the ancestors of B and A).
///
/// Returns an array of slices where each item is (slice_id, heads).
pub async fn slice_repository<NeedsProcessing, Out, R>(
    ctx: &CoreContext,
    repo: &R,
    // TODO: Move this inside repo. Need to fix backfill_derived_data first.
    skiplist_index: &SkiplistIndex,
    heads: Vec<ChangesetId>,
    needs_processing: NeedsProcessing,
    slice_size: u64,
) -> Result<Vec<(u64, Vec<ChangesetId>)>>
where
    NeedsProcessing: Fn(Vec<ChangesetId>) -> Out,
    Out: Future<Output = Result<HashSet<ChangesetId>>>,
    R: Repo,
{
    let heads = needs_processing(heads.clone()).await?;

    if skiplist_index.indexed_node_count() == 0 {
        // This skiplist index is not populated.  Generate a single
        // slice with all heads.
        info!(
            ctx.logger(),
            "Repository not sliced as skiplist index is not populated",
        );
        let heads = heads.into_iter().collect();
        return Ok(vec![(0, heads)]);
    }

    // Add any unindexed heads to the skiplist index.
    let changeset_fetcher = repo.changeset_fetcher_arc();
    for head in heads.iter() {
        skiplist_index
            .add_node(ctx, &changeset_fetcher, *head, std::u64::MAX)
            .await?;
    }

    let mut head_generation_groups: BTreeMap<u64, Vec<ChangesetId>> = BTreeMap::new();
    stream::iter(heads.into_iter().map(|csid| async move {
        let gen = repo
            .changeset_fetcher()
            .get_generation_number(ctx, csid)
            .await?;
        Ok(Some((csid, gen)))
    }))
    .buffered(100)
    .try_for_each(|maybe_csid_gen| {
        if let Some((csid, gen)) = maybe_csid_gen {
            let gen_group = (gen.value() / slice_size) * slice_size;
            head_generation_groups
                .entry(gen_group)
                .or_default()
                .push(csid);
        }
        async { Ok::<_, Error>(()) }
    })
    .await?;

    let mut slices = Vec::new();
    let mut next_log_size = 1;
    while let Some((cur_gen, mut heads)) = head_generation_groups.pop_last() {
        if slices.len() > next_log_size {
            info!(
                ctx.logger(),
                "Adding slice starting at generation {} with {} heads ({} slices queued, {} so far)",
                cur_gen,
                heads.len(),
                head_generation_groups.len(),
                slices.len(),
            );
            next_log_size *= 2;
        }
        let mut new_heads_groups = HashMap::new();
        let mut seen: HashSet<_> = heads.iter().cloned().collect();
        while let Some(csid) = heads.pop() {
            let skip_parents = match skiplist_index.get_furthest_edges(csid) {
                Some(skip_parents) => skip_parents,
                None => {
                    // Ordinarily this shouldn't happen, as the skiplist ought
                    // to refer to commits that are also in the skiplist.
                    // However, if the commit is missing from the skiplist, we
                    // can look up the parents and their generations directly.
                    parents_with_generations(ctx, repo, csid).await?
                }
            };

            for (parent, gen) in skip_parents {
                if gen.value() >= cur_gen {
                    // This commit is in the same generation group.
                    if seen.insert(parent) {
                        heads.push(parent);
                    }
                } else {
                    // This commit is in a new generation group.
                    let gen_group = (gen.value() / slice_size) * slice_size;
                    new_heads_groups.insert(parent, gen_group);
                }
            }
        }

        // Add all commits we've seen to the slice.  The heads from the start
        // of this iteration would be sufficient, however providing additional
        // changesets will allow traversal of the graph to find all commits to
        // run faster as it can fetch the parents of multiple commits at once.
        slices.push((cur_gen, seen.into_iter().collect()));

        // For each new head, check if it needs derivation, and if so, add it
        // to its generation group.
        let new_heads: Vec<_> = new_heads_groups.keys().cloned().collect();
        let underived_new_heads = needs_processing(new_heads).await?;
        for head in underived_new_heads {
            if let Some(gen_group) = new_heads_groups.get(&head) {
                head_generation_groups
                    .entry(*gen_group)
                    .or_default()
                    .push(head);
            }
        }
    }

    if !slices.is_empty() {
        info!(
            ctx.logger(),
            "Repository sliced into {} slices requiring derivation",
            slices.len()
        );
    }
    slices.reverse();

    Ok(slices)
}
