// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashSet;
use std::sync::Arc;

use errors::*;
use failure::Error;
use futures::future::{join_all, Future};
use futures::stream::{iter_ok, Stream};
use futures_ext::FutureExt;

use blobrepo::ChangesetFetcher;
use mononoke_types::{ChangesetId, Generation};

/// Attempts to fetch the generation number of the hash. Succeeds with the Generation value
/// of the node if the node exists, else fails with ErrorKind::NodeNotFound.
pub fn fetch_generation(
    changeset_fetcher: Arc<ChangesetFetcher>,
    node: ChangesetId,
) -> impl Future<Item = Generation, Error = Error> {
    changeset_fetcher.get_generation_number(node)
}

pub fn fetch_generation_and_join(
    changeset_fetcher: Arc<ChangesetFetcher>,
    node: ChangesetId,
) -> impl Future<Item = (ChangesetId, Generation), Error = Error> {
    fetch_generation(changeset_fetcher, node).map(move |gen| (node, gen))
}
/// Confirm whether or not a node with the given hash exists in the repo.
/// Succeeds with the void value () if the node exists, else fails with ErrorKind::NodeNotFound.
pub fn check_if_node_exists(
    changeset_fetcher: Arc<ChangesetFetcher>,
    node: ChangesetId,
) -> impl Future<Item = (), Error = Error> {
    changeset_fetcher
        .get_generation_number(node)
        .map(|_| ())
        .map_err(|err| ErrorKind::NodeNotFound(format!("{}", err)).into())
}

/// Convert a collection of ChangesetId to a collection of (ChangesetId, Generation)
pub fn changesets_with_generation_numbers(
    changeset_fetcher: Arc<ChangesetFetcher>,
    nodes: Vec<ChangesetId>,
) -> impl Future<Item = Vec<(ChangesetId, Generation)>, Error = Error> {
    join_all(nodes.into_iter().map({
        cloned!(changeset_fetcher);
        move |hash| fetch_generation_and_join(changeset_fetcher.clone(), hash)
    }))
}

/// Attempt to get the changeset parents of a hash node,
/// and cast into the appropriate ErrorKind if it fails
pub fn get_parents(
    changeset_fetcher: Arc<ChangesetFetcher>,
    node: ChangesetId,
) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
    changeset_fetcher.get_parents(node)
}

// Take ownership of two sets, the current 'layer' of the bfs, and all nodes seen until then.
// Produce a future which does the following computation:
// - add all nodes in the current layer to the seen set
// - get the set of parents of nodes in the current layer
// - filter out previously seen nodes from the parents
// - return the parents as the next bfs layer, and the updated seen as the new seen set
pub fn advance_bfs_layer(
    changeset_fetcher: Arc<ChangesetFetcher>,
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
        .map(move |(hash, _gen)| changeset_fetcher.get_parents(hash))
        .buffered(100)
        .map(|parents| iter_ok::<_, Error>(parents.into_iter()))
        .flatten()
        .collect()
        .and_then(move |all_parents| {
            changesets_with_generation_numbers(new_changeset_fetcher, all_parents)
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

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use async_unit;
    use context::CoreContext;
    use fixtures::linear;
    use futures::Future;
    use mononoke_types::Generation;

    use helpers::fetch_generation_and_join;
    use tests::string_to_bonsai;

    #[test]
    fn test_helpers() {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let mut ordered_hashes_oldest_to_newest = vec![
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "cb15ca4a43a59acff5388cea9648c162afde8372",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "d0a361e9022d226ae52f689667bd7d212a19cfe0",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "607314ef579bd2407752361ba1b0c1729d08b281",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "3e0e761030db6e479a7fb58b12881883f9f8c63f",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
                ),
            ];
            ordered_hashes_oldest_to_newest.reverse();

            for (i, node) in ordered_hashes_oldest_to_newest.into_iter().enumerate() {
                assert_eq!(
                    fetch_generation_and_join(repo.get_changeset_fetcher(), node)
                        .wait()
                        .unwrap(),
                    (node, Generation::new(i as u64 + 1))
                );
            }
        });
    }
}
