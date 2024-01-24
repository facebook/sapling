/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use derived_data_utils::DerivedUtils;
use futures::stream;
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use repo_derived_data::RepoDerivedDataArc;

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
                    .pending(ctx.clone(), repo.repo_derived_data_arc(), heads.to_vec())
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

/// Slices ancestors of heads into a sequence of slices for derivation.
///
/// Each slice contains a frontier of changesets within a generation range, returning
/// (slice_start, slice_frontier) corresponds to the frontier that has generations numbers
/// within [slice_start..(slice_start + slice_size)].
///
/// This allows derivation of the first slice with underived commits to begin
/// more quickly, as the rest of the repository history doesn't need to be
/// traversed.
///
/// The returned slices consist only of frontiers which haven't been derived yet
/// by the provided derivers. Slicing stops once we reach a frontier with all its
/// changesets derived.
///
/// If any of these heads are already derived then they are omitted.  Empty
/// slices are also omitted.
pub(crate) async fn slice_repository(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derivers: &[Arc<dyn DerivedUtils>],
    heads: Vec<ChangesetId>,
    slice_size: u64,
) -> Result<Vec<(u64, Vec<ChangesetId>)>> {
    Ok(repo
        .commit_graph()
        .slice_ancestors(
            ctx,
            heads,
            |heads| async move { underived_heads(ctx, repo, derivers, heads.as_slice()).await },
            slice_size,
        )
        .await?
        .into_iter()
        .map(|(slice_start, slice_heads)| (slice_start.value(), slice_heads))
        .collect())
}
