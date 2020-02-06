/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use cloned::cloned;
use context::CoreContext;
use futures::future::{join_all, Future};
use futures::stream::{iter_ok, Stream};
use futures_ext::FutureExt;

use changeset_fetcher::ChangesetFetcher;
use mononoke_types::{ChangesetId, Generation};
use reachabilityindex::errors::*;

/// Attempts to fetch the generation number of the hash. Succeeds with the Generation value
/// of the node if the node exists, else fails with ErrorKind::NodeNotFound.
pub fn fetch_generation(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    node: ChangesetId,
) -> impl Future<Item = Generation, Error = Error> {
    changeset_fetcher.get_generation_number(ctx, node)
}

pub fn fetch_generation_and_join(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    node: ChangesetId,
) -> impl Future<Item = (ChangesetId, Generation), Error = Error> {
    fetch_generation(ctx, changeset_fetcher, node).map(move |gen| (node, gen))
}
/// Confirm whether or not a node with the given hash exists in the repo.
/// Succeeds with the void value () if the node exists, else fails with ErrorKind::NodeNotFound.
pub fn check_if_node_exists(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    node: ChangesetId,
) -> impl Future<Item = (), Error = Error> {
    changeset_fetcher
        .get_generation_number(ctx, node)
        .map(|_| ())
        .map_err(|err| ErrorKind::NodeNotFound(format!("{}", err)).into())
}

/// Convert a collection of ChangesetId to a collection of (ChangesetId, Generation)
pub fn changesets_with_generation_numbers(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    nodes: Vec<ChangesetId>,
) -> impl Future<Item = Vec<(ChangesetId, Generation)>, Error = Error> {
    join_all(nodes.into_iter().map({
        cloned!(changeset_fetcher);
        move |hash| fetch_generation_and_join(ctx.clone(), changeset_fetcher.clone(), hash)
    }))
}

/// Attempt to get the changeset parents of a hash node,
/// and cast into the appropriate ErrorKind if it fails
pub fn get_parents(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    node: ChangesetId,
) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
    changeset_fetcher.get_parents(ctx, node)
}

// Take ownership of two sets, the current 'layer' of the bfs, and all nodes seen until then.
// Produce a future which does the following computation:
// - add all nodes in the current layer to the seen set
// - get the set of parents of nodes in the current layer
// - filter out previously seen nodes from the parents
// - return the parents as the next bfs layer, and the updated seen as the new seen set
pub fn advance_bfs_layer(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    curr_layer: HashSet<(ChangesetId, Generation)>,
    mut curr_seen: HashSet<(ChangesetId, Generation)>,
) -> impl Future<
    Item = (
        HashSet<(ChangesetId, Generation)>,
        HashSet<(ChangesetId, Generation)>,
    ),
    Error = Error,
> {
    let new_changeset_fetcher = changeset_fetcher.clone();
    for next_node in curr_layer.iter() {
        curr_seen.insert(next_node.clone());
    }

    iter_ok::<_, Error>(curr_layer)
        .map({
            cloned!(ctx);
            move |(hash, _gen)| changeset_fetcher.get_parents(ctx.clone(), hash)
        })
        .buffered(100)
        .map(|parents| iter_ok::<_, Error>(parents.into_iter()))
        .flatten()
        .collect()
        .and_then(move |all_parents| {
            changesets_with_generation_numbers(ctx, new_changeset_fetcher, all_parents)
        })
        .map(move |flattened_node_generation_pairs| {
            let mut next_layer = HashSet::new();
            for hash_gen_pair in flattened_node_generation_pairs.into_iter() {
                if !curr_seen.contains(&hash_gen_pair) {
                    next_layer.insert(hash_gen_pair);
                }
            }
            (next_layer, curr_seen)
        })
        .boxify()
}
