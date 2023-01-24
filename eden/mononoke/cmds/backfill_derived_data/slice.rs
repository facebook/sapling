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
use context::CoreContext;
use derived_data_utils::DerivedUtils;
use futures::stream;
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use repo_derived_data::RepoDerivedDataArc;
use skiplist::SkiplistIndex;

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
) -> Result<Vec<(u64, Vec<ChangesetId>)>> {
    slice_repository::slice_repository(
        ctx,
        repo,
        skiplist_index,
        heads,
        |heads| async move { underived_heads(ctx, repo, derivers, heads.as_slice()).await },
        slice_size,
    )
    .await
}
