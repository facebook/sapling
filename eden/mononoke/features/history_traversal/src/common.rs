/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::CoreContext;
use futures::stream;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use mononoke_types::MPath;
use reachabilityindex::ReachabilityIndex;

use crate::Repo;

/// Given a changeset and path finds all ancestors where the history for path
/// has been mutated. Given history graph:
/// a
/// |
/// b
/// |
/// c
/// |\
/// d e
/// where we are b, this will tell you if the path history for this specific
/// path was overriden at c, d or e.
///
/// Returs vector of (generation_number, changeset_id) for changesets sorted by
/// generation number.
pub(crate) async fn find_possible_mutable_ancestors(
    ctx: &CoreContext,
    repo: &impl Repo,
    csid: ChangesetId,
    path: Option<&MPath>,
) -> Result<Vec<(Generation, ChangesetId)>, Error> {
    let mutable_renames = repo.mutable_renames();
    let mutable_csids = mutable_renames
        .get_cs_ids_with_rename(ctx, path.cloned())
        .await?;
    let skiplist_index = repo.skiplist_index();
    let mut possible_mutable_ancestors: Vec<(Generation, ChangesetId)> =
        stream::iter(mutable_csids.into_iter().map(anyhow::Ok))
            .try_filter_map({
                move |mutated_at| async move {
                    // First, we filter out csids that cannot be reached from here. These
                    // are attached to mutable renames that are either descendants of us, or
                    // in a completely unrelated tree of history.
                    if skiplist_index
                        .query_reachability(ctx, &repo.changeset_fetcher_arc(), csid, mutated_at)
                        .await?
                    {
                        // We also want to grab generation here, because we're going to sort
                        // by generation and consider "most recent" candidate first
                        let cs_gen = repo
                            .changeset_fetcher()
                            .get_generation_number(ctx.clone(), mutated_at)
                            .await?;
                        Ok(Some((cs_gen, mutated_at)))
                    } else {
                        anyhow::Ok(None)
                    }
                }
            })
            .try_collect()
            .await?;
    // And turn the list of possible mutable ancestors into a stack sorted by generation
    possible_mutable_ancestors.sort_unstable_by_key(|(gen, _)| *gen);

    Ok(possible_mutable_ancestors)
}
