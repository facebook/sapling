// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{HashMap, HashSet};
use std::default::Default;
use std::sync::Arc;

use failure::Error;
use futures_ext::BoxFuture;

use blobrepo::ChangesetFetcher;
use mononoke_types::{ChangesetId, Generation};

#[derive(Eq, PartialEq, Clone, Debug)]
pub struct NodeFrontier {
    pub gen_map: HashMap<Generation, HashSet<ChangesetId>>,
}

impl Default for NodeFrontier {
    fn default() -> Self {
        Self {
            gen_map: HashMap::new(),
        }
    }
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

    pub fn len(&self) -> usize {
        self.gen_map.len()
    }

    pub fn insert(&mut self, edge_pair: (ChangesetId, Generation)) {
        self.gen_map
            .entry(edge_pair.1)
            .or_insert(HashSet::new())
            .insert(edge_pair.0);
    }

    pub fn insert_iter(&mut self, iter: impl IntoIterator<Item = (ChangesetId, Generation)>) {
        for edge_pair in iter {
            self.insert(edge_pair);
        }
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
