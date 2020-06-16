/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use context::CoreContext;
use futures::future::try_join_all;
use futures::stream::{iter, StreamExt, TryStreamExt};
use futures_util::compat::Future01CompatExt;

use changeset_fetcher::ChangesetFetcher;
use mononoke_types::{ChangesetId, Generation};
use reachabilityindex::errors::*;

/// Attempts to fetch the generation number of the hash. Succeeds with the Generation value
/// of the node if the node exists, else fails with ErrorKind::NodeNotFound.
pub async fn fetch_generation(
    ctx: &CoreContext,
    changeset_fetcher: &Arc<dyn ChangesetFetcher>,
    node: ChangesetId,
) -> Result<Generation, Error> {
    changeset_fetcher
        .get_generation_number(ctx.clone(), node)
        .compat()
        .await
}

/// Confirm whether or not a node with the given hash exists in the repo.
/// Succeeds with the void value () if the node exists, else fails with ErrorKind::NodeNotFound.
pub async fn check_if_node_exists(
    ctx: &CoreContext,
    changeset_fetcher: &Arc<dyn ChangesetFetcher>,
    node: ChangesetId,
) -> Result<(), Error> {
    changeset_fetcher
        .get_generation_number(ctx.clone(), node)
        .compat()
        .await
        .map(|_| ())
        .map_err(|err| ErrorKind::NodeNotFound(format!("{}", err)).into())
}

/// Convert a collection of ChangesetId to a collection of (ChangesetId, Generation)
pub async fn changesets_with_generation_numbers(
    ctx: &CoreContext,
    changeset_fetcher: &Arc<dyn ChangesetFetcher>,
    nodes: Vec<ChangesetId>,
) -> Result<Vec<(ChangesetId, Generation)>, Error> {
    try_join_all(nodes.into_iter().map(|node| async move {
        Ok((node, fetch_generation(ctx, changeset_fetcher, node).await?))
    }))
    .await
}

/// Attempt to get the changeset parents of a hash node,
/// and cast into the appropriate ErrorKind if it fails
pub async fn get_parents(
    ctx: &CoreContext,
    changeset_fetcher: &Arc<dyn ChangesetFetcher>,
    node: ChangesetId,
) -> Result<Vec<ChangesetId>, Error> {
    changeset_fetcher
        .get_parents(ctx.clone(), node)
        .compat()
        .await
}

// Take ownership of two sets, the current 'layer' of the bfs, and all nodes seen until then.
// Produce a future which does the following computation:
// - add all nodes in the current layer to the seen set
// - get the set of parents of nodes in the current layer
// - filter out previously seen nodes from the parents
// - return the parents as the next bfs layer, and the updated seen as the new seen set
pub async fn advance_bfs_layer(
    ctx: &CoreContext,
    changeset_fetcher: &Arc<dyn ChangesetFetcher>,
    curr_layer: HashSet<(ChangesetId, Generation)>,
    mut curr_seen: HashSet<(ChangesetId, Generation)>,
) -> Result<
    (
        HashSet<(ChangesetId, Generation)>,
        HashSet<(ChangesetId, Generation)>,
    ),
    Error,
> {
    for next_node in curr_layer.iter() {
        curr_seen.insert(next_node.clone());
    }

    let parent_gen: Vec<_> = iter(curr_layer)
        .map(|(hash, _gen)| get_parents(ctx, changeset_fetcher, hash))
        .buffer_unordered(100)
        .map_ok(|parents| iter(parents.into_iter().map(Ok::<_, Error>)))
        .try_flatten()
        .and_then(|parent| async move {
            Ok((
                parent,
                fetch_generation(ctx, changeset_fetcher, parent).await?,
            ))
        })
        .try_collect()
        .await?;

    let mut next_layer = HashSet::new();
    for hash_gen_pair in parent_gen.into_iter() {
        if !curr_seen.contains(&hash_gen_pair) {
            next_layer.insert(hash_gen_pair);
        }
    }
    Ok((next_layer, curr_seen))
}
