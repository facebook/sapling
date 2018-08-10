// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use failure::Error;
use futures_ext::BoxFuture;

use blobrepo::BlobRepo;
use mercurial_types::HgNodeHash;
use mononoke_types::Generation;

#[allow(dead_code)]
pub struct NodeFrontier {
    pub gen_map: HashMap<Generation, HashSet<HgNodeHash>>,
}

#[allow(dead_code)]
impl NodeFrontier {
    pub fn new(gen_map: HashMap<Generation, HashSet<HgNodeHash>>) -> Self {
        NodeFrontier { gen_map }
    }
}

/// Trait for any method of supporting reachability queries
pub trait ReachabilityIndex {
    /// Return a Future for whether the src node can reach the dst node
    fn query_reachability(
        &self,
        repo: Arc<BlobRepo>,
        src: HgNodeHash,
        dst: HgNodeHash,
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
        repo: Arc<BlobRepo>,
        node_frontier: NodeFrontier,
        gen: Generation,
    ) -> BoxFuture<NodeFrontier, Error>;
}
