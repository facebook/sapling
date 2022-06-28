/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map;
use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Error;
use async_trait::async_trait;
use auto_impl::auto_impl;
use maplit::hashmap;
use maplit::hashset;

use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
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
        let mut generations = UniqueHeap::new();
        for (gen, _) in input.iter() {
            generations.push(gen.clone());
        }

        Self {
            gen_map: input,
            generations,
        }
    }

    pub async fn new_from_single_node(
        ctx: &CoreContext,
        changeset_fetcher: ArcChangesetFetcher,
        node: ChangesetId,
    ) -> Result<Self, Error> {
        let gen = changeset_fetcher
            .get_generation_number(ctx.clone(), node)
            .await?;
        Ok(Self::new(hashmap! {gen => hashset!{node}}))
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

    pub fn remove_max_gen(&mut self) -> Option<(Generation, HashSet<ChangesetId>)> {
        let max_gen = self.generations.pop()?;
        Some((
            max_gen,
            self.gen_map
                .remove(&max_gen)
                .expect("inconsistent frontier state"),
        ))
    }

    pub fn len(&self) -> usize {
        self.gen_map.len()
    }

    /// Returns a new node frontier that contains only the nodes that are
    /// present in both: `self` and `other`.
    pub fn intersection(&self, other: &NodeFrontier) -> NodeFrontier {
        let mut res = Self::new(hashmap! {});
        for (gen, self_changesets) in self.gen_map.iter() {
            if let Some(other_changesets) = other.gen_map.get(gen) {
                let res_changesets: HashSet<_> = self_changesets
                    .intersection(other_changesets)
                    .cloned()
                    .collect();
                if !res_changesets.is_empty() {
                    res.gen_map.insert(gen.clone(), res_changesets.clone());
                    res.generations.push(gen.clone());
                }
            }
        }
        res
    }

    /// Simple iterator over all nodes in the frontier, doesn't guarantee any ordering.
    pub fn iter(&self) -> impl Iterator<Item = (&ChangesetId, Generation)> {
        self.gen_map.iter().flat_map(|(gen, changesets)| {
            changesets
                .iter()
                .map(move |changeset| (changeset, gen.clone()))
        })
    }
}

/// Trait for any method of supporting reachability queries
#[async_trait]
pub trait ReachabilityIndex: Send + Sync {
    /// Return a Future for whether the src node can reach the dst node
    async fn query_reachability(
        &self,
        ctx: &CoreContext,
        repo: &ArcChangesetFetcher,
        src: ChangesetId,
        dst: ChangesetId,
    ) -> Result<bool, Error>;
}

/// Trait for any method supporting computing an "LCA hint"
#[async_trait]
#[auto_impl(Arc)]
pub trait LeastCommonAncestorsHint: Send + Sync {
    /// Return a Future for an advanced frontier of ancestors from a set of nodes.
    /// Given a set "nodes", and a maximum generation "gen",
    /// return a set of nodes "C" which satisfies:
    /// - Max generation number in "C" is <= gen
    /// - Any ancestor of "nodes" with generation <= gen is also an ancestor of "C"
    async fn lca_hint(
        &self,
        ctx: &CoreContext,
        repo: &ArcChangesetFetcher,
        node_frontier: NodeFrontier,
        gen: Generation,
    ) -> Result<NodeFrontier, Error>;

    /// Check if `ancestor` changeset is an ancestor of `descendant` changeset
    /// Note that a changeset IS NOT its own ancestor
    async fn is_ancestor(
        &self,
        ctx: &CoreContext,
        repo: &ArcChangesetFetcher,
        ancestor: ChangesetId,
        descendant: ChangesetId,
    ) -> Result<bool, Error>;
}
