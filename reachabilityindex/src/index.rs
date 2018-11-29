// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use failure::Error;
use futures::Future;
use futures::future::{join_all, loop_fn, ok, Loop};
use futures_ext::{BoxFuture, FutureExt};

use blobrepo::ChangesetFetcher;
use mononoke_types::{ChangesetId, Generation};

#[derive(Eq, PartialEq, Clone, Debug)]
pub struct NodeFrontier {
    pub gen_map: HashMap<Generation, HashSet<ChangesetId>>,
}

impl NodeFrontier {
    pub fn new(gen_map: HashMap<Generation, HashSet<ChangesetId>>) -> Self {
        NodeFrontier { gen_map }
    }

    pub fn from_pairs(node_gen_pairs: Vec<(ChangesetId, Generation)>) -> Self {
        let mut gen_map = HashMap::new();
        for (node, gen) in node_gen_pairs.into_iter() {
            gen_map.entry(gen).or_insert(HashSet::new()).insert(node);
        }
        NodeFrontier { gen_map }
    }

    pub fn is_empty(&self) -> bool {
        self.gen_map.is_empty()
    }

    pub fn max_gen(&self) -> Option<Generation> {
        self.gen_map.keys().max().cloned()
    }
}

/// Trait for any method of supporting reachability queries
pub trait ReachabilityIndex {
    /// Return a Future for whether the src node can reach the dst node
    fn query_reachability(
        &self,
        repo: Arc<ChangesetFetcher>,
        src: ChangesetId,
        dst: ChangesetId,
    ) -> BoxFuture<bool, Error>;
}

/// Trait for any method supporting computing an "LCA hint"
pub trait LeastCommonAncestorsHint {
    /// Return a Future for an advanced frontier of ancestors from a set of nodes.
    /// Given a set "nodes", and a maximum generation "gen",
    /// return a set of nodes "C" which satisfies:
    /// - Max generation number in "C" is <= gen
    /// - Any ancestor of "nodes" with generation <= gen is also an ancestor of "C"
    fn lca_hint(
        &self,
        repo: Arc<ChangesetFetcher>,
        node_frontier: NodeFrontier,
        gen: Generation,
    ) -> BoxFuture<NodeFrontier, Error>;
}

pub struct SimpleLcaHint {}

impl LeastCommonAncestorsHint for SimpleLcaHint {
    fn lca_hint(
        &self,
        changeset_fetcher: Arc<ChangesetFetcher>,
        node_frontier: NodeFrontier,
        gen: Generation,
    ) -> BoxFuture<NodeFrontier, Error> {
        loop_fn(
            node_frontier,
            move |mut node_frontier: NodeFrontier| match node_frontier.max_gen() {
                Some(val) if val <= gen => ok(Loop::Break(node_frontier)).boxify(),
                Some(val) => {
                    let cs_ids = node_frontier.gen_map.remove(&val).unwrap();
                    join_all(cs_ids.into_iter().map({
                        cloned!(changeset_fetcher);
                        move |cs_id| {
                            changeset_fetcher.get_parents(cs_id).and_then({
                                cloned!(changeset_fetcher);
                                move |parents| {
                                    join_all(parents.into_iter().map(move |p| {
                                        changeset_fetcher
                                            .get_generation_number(p)
                                            .map(move |gen_num| (gen_num, p))
                                    }))
                                }
                            })
                        }
                    })).map(move |all_parents| {
                        all_parents.into_iter().flatten().collect::<Vec<_>>()
                    })
                        .map(move |gen_cs| {
                            for (gen_num, cs) in gen_cs {
                                node_frontier
                                    .gen_map
                                    .entry(gen_num)
                                    .or_insert(HashSet::new())
                                    .insert(cs);
                            }
                            Loop::Continue(node_frontier)
                        })
                        .boxify()
                }
                None => ok(Loop::Break(node_frontier)).boxify(),
            },
        ).boxify()
    }
}
