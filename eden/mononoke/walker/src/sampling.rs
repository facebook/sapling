/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{EdgeType, Node, NodeData, NodeType, WrappedPath};
use crate::state::{StepStats, WalkStateCHashMap};
use crate::walk::{OutgoingEdge, ResolvedNode, WalkVisitor};

use context::{CoreContext, SamplingKey};
use dashmap::DashMap;
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PathTrackingRoute {
    // The path we reached this by
    pub path: Option<WrappedPath>,
}

impl PathTrackingRoute {
    fn evolve_path<'a>(
        route: Option<&'a Self>,
        from_step: Option<&'a WrappedPath>,
        target: &'a Node,
    ) -> Option<&'a WrappedPath> {
        match from_step {
            // Step has set explicit path, e.g. bonsai file
            Some(from_step) => Some(from_step),
            None => match target.stats_path() {
                // Path is part of node identity
                Some(from_node) => Some(from_node),
                // No per-node path, so use the route
                None => route.and_then(|r| r.path.as_ref()),
            },
        }
    }

    fn evolve(route: Option<Self>, path: Option<&WrappedPath>, target: &Node) -> Self {
        let new_path = PathTrackingRoute::evolve_path(route.as_ref(), path, target);

        // reuse same route if possible
        if new_path == route.as_ref().and_then(|r| r.path.as_ref()) {
            if let Some(route) = route {
                return route;
            }
        }

        Self {
            path: new_path.cloned(),
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
            let should_sample = match self.sample_rate {
                0 => false,
                1 => true,
                sample_rate => {
                    let sampling_fingerprint =
                        PathTrackingRoute::evolve_path(route, step.path.as_ref(), &step.target)
                            .map_or_else(
                                || step.target.sampling_fingerprint(),
                                |r| r.sampling_fingerprint(),
                            );
                    sampling_fingerprint.map_or(true, |fp| fp % sample_rate == 0)
                }
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
        let route = PathTrackingRoute::evolve(route, current.path.as_ref(), &current.node);
        let (vout, _inner_route, outgoing) = self.inner.visit(ctx, current, Some(()), outgoing);
        (vout, route, outgoing)
    }
}

// Super simple sampling visitor impl for scrubbing
impl<T> WalkVisitor<(Node, Option<NodeData>, Option<StepStats>), ()> for SamplingWalkVisitor<T>
where
    T: Default,
{
    fn start_step(
        &self,
        mut ctx: CoreContext,
        route: Option<&()>,
        step: &OutgoingEdge,
    ) -> CoreContext {
        if self.sample_node_types.contains(&step.target.get_type()) {
            let should_sample = match self.sample_rate {
                0 => false,
                1 => true,
                sample_rate => step
                    .target
                    .sampling_fingerprint()
                    .map_or(true, |fp| fp % sample_rate == 0),
            };

            if should_sample {
                ctx = ctx.clone_and_sample(SamplingKey::new());
                if let Some(k) = ctx.sampling_key() {
                    self.sampler.start_node(*k, step.target.clone())
                }
            }
        }
        self.inner.start_step(ctx, route.map(|_| &()), step)
    }

    fn visit(
        &self,
        ctx: &CoreContext,
        current: ResolvedNode,
        route: Option<()>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (
        (Node, Option<NodeData>, Option<StepStats>),
        (),
        Vec<OutgoingEdge>,
    ) {
        self.inner.visit(ctx, current, route, outgoing)
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
