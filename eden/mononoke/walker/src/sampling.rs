/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{EdgeType, Node, NodeData, NodeType};
use crate::state::{StepStats, WalkStateCHashMap};
use crate::walk::{OutgoingEdge, ResolvedNode, WalkVisitor};

use context::{CoreContext, SamplingKey};
use dashmap::DashMap;
use mononoke_types::MPath;
use std::{collections::HashSet, sync::Arc};

#[derive(Debug)]
pub struct SamplingWalkVisitor<T> {
    inner: WalkStateCHashMap,
    sample_node_types: HashSet<NodeType>,
    sampler: Arc<NodeSamplingHandler<T>>,
    sample_rate: u64,
}

impl<T> SamplingWalkVisitor<T> {
    pub fn new(
        include_node_types: HashSet<NodeType>,
        include_edge_types: HashSet<EdgeType>,
        sample_node_types: HashSet<NodeType>,
        sampler: Arc<NodeSamplingHandler<T>>,
        sample_rate: u64,
    ) -> Self {
        Self {
            inner: WalkStateCHashMap::new(include_node_types, include_edge_types),
            sample_node_types,
            sampler,
            sample_rate,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WrappedPath {
    Root,
    NonRoot(Arc<MPath>),
}

impl WrappedPath {
    pub fn as_ref(&self) -> Option<&Arc<MPath>> {
        match self {
            WrappedPath::Root => None,
            WrappedPath::NonRoot(path) => Some(&path),
        }
    }
}

impl From<Option<MPath>> for WrappedPath {
    fn from(mpath: Option<MPath>) -> Self {
        match mpath {
            Some(mpath) => WrappedPath::NonRoot(Arc::new(mpath)),
            None => WrappedPath::Root,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PathTrackingRoute {
    // The path we reached this by.  Some(None) means root.
    pub path: Option<WrappedPath>,
}

impl PathTrackingRoute {
    fn sampling_fingerprint(&self) -> Option<u64> {
        self.path
            .as_ref()
            .and_then(|o| o.as_ref().map(|p| p.get_path_hash().sampling_fingerprint()))
    }
}

// A non-root path
impl From<MPath> for PathTrackingRoute {
    fn from(mpath: MPath) -> Self {
        Self {
            path: Some(WrappedPath::from(Some(mpath))),
        }
    }
}

// A path that might be root
impl From<Option<MPath>> for PathTrackingRoute {
    fn from(mpath: Option<MPath>) -> Self {
        Self {
            path: Some(WrappedPath::from(mpath)),
        }
    }
}

impl<T> WalkVisitor<(Node, Option<NodeData>, Option<StepStats>), PathTrackingRoute>
    for SamplingWalkVisitor<T>
where
    T: Default,
{
    fn start_step(
        &self,
        mut ctx: CoreContext,
        route: Option<&PathTrackingRoute>,
        step: &OutgoingEdge,
    ) -> CoreContext {
        if self.sample_node_types.contains(&step.target.get_type()) {
            let should_sample = if self.sample_rate == 1 {
                true
            } else if self.sample_rate == 0 {
                false
            } else {
                let sampling_fingerprint = match step.target.stats_path() {
                    Some(path_opt) => path_opt
                        .as_ref()
                        .map(|p| p.get_path_hash().sampling_fingerprint()),
                    None => match route {
                        Some(route) => route.sampling_fingerprint(),
                        // TODO, sample non-path node types
                        None => None,
                    },
                };

                sampling_fingerprint
                    .map(|fp| fp % self.sample_rate == 0)
                    .unwrap_or(true)
            };

            if should_sample {
                ctx = ctx.clone_and_sample(SamplingKey::new());
                ctx.sampling_key()
                    .map(|k| self.sampler.start_node(*k, step.target.clone()));
            }
        }
        self.inner.start_step(ctx, route.map(|_| &()), step)
    }

    fn visit(
        &self,
        ctx: &CoreContext,
        current: ResolvedNode,
        route: Option<PathTrackingRoute>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (
        (Node, Option<NodeData>, Option<StepStats>),
        PathTrackingRoute,
        Vec<OutgoingEdge>,
    ) {
        let route = match current.node.stats_path() {
            Some(mpath) => PathTrackingRoute::from(mpath.cloned()),
            None => match route {
                Some(route) => route,
                None => PathTrackingRoute::default(),
            },
        };

        let (vout, _inner_route, outgoing) = self.inner.visit(ctx, current, Some(()), outgoing);
        (vout, route, outgoing)
    }
}

#[derive(Debug)]
pub struct NodeSamplingHandler<T> {
    // T can keep a one to many mapping, e.g. some nodes like
    // chunked files have multiple blobstore keys
    inflight: DashMap<SamplingKey, T>,
    // 1:1 relationship, each node has one SamplingKey
    inflight_reverse: DashMap<Node, SamplingKey>,
}

impl<T> NodeSamplingHandler<T>
where
    T: Default,
{
    pub fn new() -> Self {
        Self {
            inflight: DashMap::new(),
            inflight_reverse: DashMap::new(),
        }
    }

    // Called from the blobstore sampling callback
    pub fn inflight(&self) -> &DashMap<SamplingKey, T> {
        &self.inflight
    }

    // Called from the visitor start_step
    pub fn start_node(&self, key: SamplingKey, node: Node) {
        self.inflight.insert(key, T::default());
        self.inflight_reverse.insert(node, key);
    }

    pub fn is_sampling(&self, node: &Node) -> bool {
        self.inflight_reverse.contains_key(node)
    }

    // Needs to be called to stop tracking the node and thus free memory.
    // Can be called from the vistor visit, or in the stream processing
    // walk output.
    pub fn complete_node(&self, node: &Node) -> Option<T> {
        let reverse_mapping = self.inflight_reverse.remove(node);
        reverse_mapping
            .as_ref()
            .and_then(|(_k, sample_key)| self.inflight.remove(sample_key))
            .map(|(_k, v)| v)
    }
}
