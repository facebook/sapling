/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{hash_map, HashMap, HashSet};
use std::default::Default;
use std::iter::{Extend, FromIterator};
use std::sync::Arc;

use failure_ext::Error;
use futures_ext::BoxFuture;

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::{ChangesetId, Generation};
use uniqueheap::UniqueHeap;

#[derive(Clone, Debug)]
pub struct NodeFrontier {
    gen_map: HashMap<Generation, HashSet<ChangesetId>>,
    generations: UniqueHeap<Generation>,
}

impl PartialEq for NodeFrontier {
    fn eq(&self, other: &NodeFrontier) -> bool {
        self.gen_map == other.gen_map
    }
}
impl Eq for NodeFrontier {}

impl Default for NodeFrontier {
    fn default() -> Self {
        Self {
            gen_map: HashMap::new(),
            generations: UniqueHeap::new(),
        }
    }
}

impl IntoIterator for NodeFrontier {
    type Item = (Generation, HashSet<ChangesetId>);
    type IntoIter = hash_map::IntoIter<Generation, HashSet<ChangesetId>>;

    fn into_iter(self) -> Self::IntoIter {
        self.gen_map.into_iter()
    }
}

impl Extend<(ChangesetId, Generation)> for NodeFrontier {
    fn extend<T: IntoIterator<Item = (ChangesetId, Generation)>>(&mut self, iter: T) {
        for edge_pair in iter {
            self.insert(edge_pair);
        }
    }
}

impl FromIterator<(ChangesetId, Generation)> for NodeFrontier {
    fn from_iter<I: IntoIterator<Item = (ChangesetId, Generation)>>(iter: I) -> Self {
        let mut frontier = NodeFrontier::default();
        frontier.extend(iter);
        frontier
    }
}

impl NodeFrontier {
    pub fn new(input: HashMap<Generation, HashSet<ChangesetId>>) -> Self {
        let mut gen_map = HashMap::new();
        let mut generations = UniqueHeap::new();
        for (gen, set) in input {
            gen_map.insert(gen, set);
            generations.push(gen);
        }

        Self {
            gen_map,
            generations,
        }
    }

    pub fn get(&self, gen: &Generation) -> Option<&HashSet<ChangesetId>> {
        self.gen_map.get(gen)
    }

    pub fn insert(&mut self, (node, gen): (ChangesetId, Generation)) {
        self.generations.push(gen);
        self.gen_map
            .entry(gen)
            .or_insert(HashSet::new())
            .insert(node);
    }

    pub fn get_all_changesets_for_gen_num(&self, gen: Generation) -> Option<&HashSet<ChangesetId>> {
        self.gen_map.get(&gen)
    }

    pub fn is_empty(&self) -> bool {
        self.gen_map.is_empty()
    }

    pub fn max_gen(&self) -> Option<Generation> {
        self.generations.peek().cloned()
    }

    pub fn remove_max_gen(&mut self) -> Option<HashSet<ChangesetId>> {
        let max_gen = self.generations.pop()?;
        Some(
            self.gen_map
                .remove(&max_gen)
                .expect("inconsistent frontier state"),
        )
    }

    pub fn len(&self) -> usize {
        self.gen_map.len()
    }
}

/// Trait for any method of supporting reachability queries
pub trait ReachabilityIndex {
    /// Return a Future for whether the src node can reach the dst node
    fn query_reachability(
        &self,
        ctx: CoreContext,
        repo: Arc<dyn ChangesetFetcher>,
        src: ChangesetId,
        dst: ChangesetId,
    ) -> BoxFuture<bool, Error>;
}

/// Trait for any method supporting computing an "LCA hint"
pub trait LeastCommonAncestorsHint: Send + Sync {
    /// Return a Future for an advanced frontier of ancestors from a set of nodes.
    /// Given a set "nodes", and a maximum generation "gen",
    /// return a set of nodes "C" which satisfies:
    /// - Max generation number in "C" is <= gen
    /// - Any ancestor of "nodes" with generation <= gen is also an ancestor of "C"
    fn lca_hint(
        &self,
        ctx: CoreContext,
        repo: Arc<dyn ChangesetFetcher>,
        node_frontier: NodeFrontier,
        gen: Generation,
    ) -> BoxFuture<NodeFrontier, Error>;

    /// Check if `ancestor` changeset is an ancestor of `descendant` changeset
    /// Note that a changeset IS NOT its own ancestor
    fn is_ancestor(
        &self,
        ctx: CoreContext,
        repo: Arc<dyn ChangesetFetcher>,
        ancestor: ChangesetId,
        descendant: ChangesetId,
    ) -> BoxFuture<bool, Error>;
}
