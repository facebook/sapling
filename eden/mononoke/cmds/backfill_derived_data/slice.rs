/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use context::CoreContext;
use derived_data_utils::DerivedUtils;
use futures::stream;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use skiplist::SkiplistIndex;
use slog::info;

/// Determine which heads are underived in any of the derivers.
async fn underived_heads(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derivers: &[Arc<dyn DerivedUtils>],
    heads: &[ChangesetId],
) -> Result<HashSet<ChangesetId>> {
    derivers
        .iter()
        .map(|deriver| async move {
            Ok::<_, Error>(stream::iter(
                deriver
                    .pending(ctx.clone(), repo.clone(), heads.to_vec())
                    .await?
                    .into_iter()
                    .map(Ok::<_, Error>),
            ))
        })
        .collect::<FuturesUnordered<_>>()
        .try_flatten()
        .try_collect::<HashSet<_>>()
        .await
}

/// If skiplist parents are not available, fetch the parents and their
/// generation from the repo.
async fn parents_with_generations(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: ChangesetId,
) -> Result<Vec<(ChangesetId, Generation)>> {
    let parents = repo
        .get_changeset_parents_by_bonsai(ctx.clone(), csid)
        .await?;
    let parents_with_generations =
        stream::iter(parents.into_iter().map(|parent_csid| async move {
            match repo.get_generation_number(ctx.clone(), parent_csid).await? {
                Some(gen) => Ok(Some((parent_csid, gen))),
                None => Err(anyhow!(
                    "Could not find generation number for commit {} parent {}",
                    csid,
                    parent_csid
                )),
            }
        }))
        .buffered(100)
        .try_filter_map(|maybe_csid_gen| async move { Ok::<_, Error>(maybe_csid_gen) })
        .try_collect::<Vec<_>>()
        .await?;
    Ok(parents_with_generations)
}

/// Slice a respository into a sequence of slices for derivation.
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
/// haven't been derived by the provided derivers.  Slicing stops once
/// all derived commits are reached.
///
/// For example, given a repository where the skiplists have the structure:
///
///     E (gen 450)
///     :
///     D (gen 350)
///     :
///     : C (gen 275)
///     :/
///     B (gen 180)
///     :
///     A (gen 1)
///
/// And a slice size of 200, this function will generate slices:
///
///     (0, [A, B])
///     (200, [C, D])
///     (400, [E])
///
/// If any of these heads are already derived then they are omitted.  Empty
/// slices are also omitted.
///
/// This allows derivation of the first slice with underived commits to begin
/// more quickly, as the rest of the repository history doesn't need to be
/// traversed (just the ancestors of B and A).
///
/// Returns the number of slices, and an iterator where each item is
/// (slice_id, heads).
pub(crate) async fn slice_repository(
    ctx: &CoreContext,
    repo: &BlobRepo,
    skiplist_index: &SkiplistIndex,
    derivers: &[Arc<dyn DerivedUtils>],
    heads: Vec<ChangesetId>,
    slice_size: u64,
) -> Result<(usize, impl Iterator<Item = (u64, Vec<ChangesetId>)>)> {
    let heads = underived_heads(ctx, repo, derivers, heads.as_slice()).await?;

    if skiplist_index.indexed_node_count() == 0 {
        // This skiplist index is not populated.  Generate a single
        // slice with all heads.
        info!(
            ctx.logger(),
            "Repository not sliced as skiplist index is not populated",
        );
        let heads = heads.into_iter().collect();
        return Ok((1, vec![(0, heads)].into_iter().rev()));
    }

    // Add any unindexed heads to the skiplist index.
    let changeset_fetcher = repo.get_changeset_fetcher();
    for head in heads.iter() {
        skiplist_index
            .add_node(ctx, &changeset_fetcher, *head, std::u64::MAX)
            .await?;
    }

    let mut head_generation_groups: BTreeMap<u64, Vec<ChangesetId>> = BTreeMap::new();
    stream::iter(heads.into_iter().map(|csid| async move {
        match repo.get_generation_number(ctx.clone(), csid).await? {
            Some(gen) => Ok(Some((csid, gen))),
            None => Err(anyhow!(
                "Could not find generation number for head {}",
                csid
            )),
        }
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
    while let Some((cur_gen, mut heads)) = head_generation_groups.pop_last() {
        info!(
            ctx.logger(),
            "Adding slice starting at generation {} with {} heads ({} slices queued)",
            cur_gen,
            heads.len(),
            head_generation_groups.len()
        );
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
        let underived_new_heads =
            underived_heads(ctx, repo, derivers, new_heads.as_slice()).await?;
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

    Ok((slices.len(), slices.into_iter().rev()))
}
