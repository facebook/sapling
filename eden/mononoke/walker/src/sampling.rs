/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{EdgeType, Node, NodeData, NodeType, WrappedPath};
use crate::state::{StepStats, WalkStateCHashMap};
use crate::walk::{OutgoingEdge, WalkVisitor};

use context::{CoreContext, SamplingKey};
use dashmap::DashMap;
use mononoke_types::datetime::DateTime;
use regex::Regex;
use std::{collections::HashSet, fmt, hash, sync::Arc};

pub trait SampleTrigger<K> {
    fn map_keys(&self, key: SamplingKey, walk_key: K);
}

#[derive(Debug)]
pub struct SamplingWalkVisitor<T> {
    inner: WalkStateCHashMap,
    sample_node_types: HashSet<NodeType>,
    sample_path_regex: Option<Regex>,
    sampler: Arc<T>,
    sample_rate: u64,
    sample_offset: u64,
}

impl<T> SamplingWalkVisitor<T> {
    pub fn new(
        include_node_types: HashSet<NodeType>,
        include_edge_types: HashSet<EdgeType>,
        sample_node_types: HashSet<NodeType>,
        sample_path_regex: Option<Regex>,
        sampler: Arc<T>,
        sample_rate: u64,
        sample_offset: u64,
    ) -> Self {
        Self {
            inner: WalkStateCHashMap::new(include_node_types, include_edge_types),
            sample_node_types,
            sample_path_regex,
            sampler,
            sample_rate,
            sample_offset,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PathTrackingRoute {
    // The path we reached this by
    pub path: Option<WrappedPath>,
    /// When did this route see this path was updated.
    /// Taken from the last bonsai or hg changset stepped through.
    pub mtime: Option<DateTime>,
}

// Only certain node types can have repo paths associated
fn filter_repo_path(node_type: NodeType, path: Option<&'_ WrappedPath>) -> Option<&'_ WrappedPath> {
    match node_type {
        NodeType::Root => None,
        // Bonsai
        NodeType::Bookmark => None,
        NodeType::BonsaiChangeset => None,
        NodeType::BonsaiHgMapping => None,
        NodeType::BonsaiPhaseMapping => None,
        NodeType::PublishedBookmarks => None,
        NodeType::BonsaiFsnodeMapping => None,
        // Hg
        NodeType::HgBonsaiMapping => None,
        NodeType::HgChangeset => None,
        NodeType::HgManifest => path,
        NodeType::HgFileEnvelope => path,
        NodeType::HgFileNode => path,
        // Content
        NodeType::FileContent => path,
        NodeType::FileContentMetadata => path,
        NodeType::AliasContentMapping => path,
        // Derived Data
        NodeType::Fsnode => path,
    }
}

impl PathTrackingRoute {
    fn evolve_path<'a>(
        from_route: Option<&'a WrappedPath>,
        from_step: Option<&'a WrappedPath>,
        target: &'a Node,
    ) -> Option<&'a WrappedPath> {
        match from_step {
            // Step has set explicit path, e.g. bonsai file
            Some(from_step) => Some(from_step),
            None => match target.stats_path() {
                // Path is part of node identity
                Some(from_node) => Some(from_node),
                // No per-node path, so use the route, filtering out nodes that can't have repo paths
                None => filter_repo_path(target.get_type(), from_route),
            },
        }
    }

    fn evolve(
        route: Option<Self>,
        path: Option<&WrappedPath>,
        target: &Node,
        mtime: Option<&DateTime>,
    ) -> Self {
        let existing_path = route.as_ref().and_then(|r| r.path.as_ref());
        let existing_mtime = route.as_ref().and_then(|r| r.mtime.as_ref());
        let new_path = PathTrackingRoute::evolve_path(existing_path, path, target);

        // reuse same route if possible
        if new_path == existing_path && (mtime.is_none() || mtime == existing_mtime) {
            if let Some(route) = route {
                return route;
            }
        }

        Self {
            path: new_path.cloned(),
            mtime: if mtime.is_none() {
                route.and_then(|r| r.mtime)
            } else {
                mtime.cloned()
            },
        }
    }
}

// Map the key type so progress reporting works
impl<'a> From<&'a (Node, Option<WrappedPath>)> for &'a Node {
    fn from((n, _p): &'a (Node, Option<WrappedPath>)) -> &'a Node {
        n
    }
}

impl<T>
    WalkVisitor<
        (
            (Node, Option<WrappedPath>),
            (Option<DateTime>, Option<NodeData>),
            Option<StepStats>,
        ),
        PathTrackingRoute,
    > for SamplingWalkVisitor<T>
where
    T: SampleTrigger<(Node, Option<WrappedPath>)>,
{
    fn start_step(
        &self,
        mut ctx: CoreContext,
        route: Option<&PathTrackingRoute>,
        step: &OutgoingEdge,
    ) -> CoreContext {
        if self.sample_node_types.contains(&step.target.get_type()) {
            let repo_path = PathTrackingRoute::evolve_path(
                route.and_then(|r| r.path.as_ref()),
                step.path.as_ref(),
                &step.target,
            );
            if self.sample_path_regex.as_ref().map_or_else(
                || true,
                |re| match repo_path {
                    None => false,
                    Some(repo_path) => re.is_match(&repo_path.to_string()),
                },
            ) {
                let should_sample = match self.sample_rate {
                    0 => false,
                    1 => true,
                    sample_rate => {
                        let sampling_fingerprint = repo_path.map_or_else(
                            || step.target.sampling_fingerprint(),
                            |r| r.sampling_fingerprint(),
                        );
                        sampling_fingerprint.map_or(self.sample_offset % sample_rate == 0, |fp| {
                            (fp + self.sample_offset) % sample_rate == 0
                        })
                    }
                };

                if should_sample {
                    let sampling_key = SamplingKey::new();
                    ctx = ctx.clone_and_sample(sampling_key);
                    self.sampler
                        .map_keys(sampling_key, (step.target.clone(), repo_path.cloned()));
                }
            }
        }
        self.inner.start_step(ctx, route.map(|_| &()), step)
    }

    fn visit(
        &self,
        ctx: &CoreContext,
        resolved: OutgoingEdge,
        node_data: Option<NodeData>,
        route: Option<PathTrackingRoute>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (
        (
            (Node, Option<WrappedPath>),
            (Option<DateTime>, Option<NodeData>),
            Option<StepStats>,
        ),
        PathTrackingRoute,
        Vec<OutgoingEdge>,
    ) {
        let inner_route = route.as_ref().map(|_| ());

        let mtime = match &node_data {
            Some(NodeData::BonsaiChangeset(bcs)) => {
                if let Some(committer_date) = bcs.committer_date() {
                    Some(committer_date)
                } else {
                    Some(bcs.author_date())
                }
            }
            Some(NodeData::HgChangeset(hg_cs)) => Some(hg_cs.time()),
            _ => None,
        };

        let route =
            PathTrackingRoute::evolve(route, resolved.path.as_ref(), &resolved.target, mtime);
        let ((n, nd, stats), _inner_route, outgoing) =
            self.inner
                .visit(ctx, resolved, node_data, inner_route, outgoing);

        (
            ((n, route.path.clone()), (route.mtime.clone(), nd), stats),
            route,
            outgoing,
        )
    }
}

// Super simple sampling visitor impl for scrubbing
impl<T> WalkVisitor<(Node, Option<NodeData>, Option<StepStats>), ()> for SamplingWalkVisitor<T>
where
    T: SampleTrigger<Node>,
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
                    .map_or(self.sample_offset % sample_rate == 0, |fp| {
                        (fp + self.sample_offset) % sample_rate == 0
                    }),
            };

            if should_sample {
                let sampling_key = SamplingKey::new();
                ctx = ctx.clone_and_sample(sampling_key);
                self.sampler.map_keys(sampling_key, step.target.clone());
            }
        }
        self.inner.start_step(ctx, route.map(|_| &()), step)
    }

    fn visit(
        &self,
        ctx: &CoreContext,
        resolved: OutgoingEdge,
        node_data: Option<NodeData>,
        route: Option<()>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (
        (Node, Option<NodeData>, Option<StepStats>),
        (),
        Vec<OutgoingEdge>,
    ) {
        self.inner.visit(ctx, resolved, node_data, route, outgoing)
    }
}

// Map from a Sampling Key the sample type T
// And from a graph level step S to the sampling key
#[derive(Debug)]
pub struct WalkSampleMapping<S, T>
where
    S: Eq + fmt::Debug + hash::Hash,
{
    // T can keep a one to many mapping, e.g. some nodes like
    // chunked files have multiple blobstore keys
    inflight: DashMap<SamplingKey, T>,
    // 1:1 relationship, each step has one SamplingKey
    inflight_reverse: DashMap<S, SamplingKey>,
}

impl<T> SampleTrigger<Node> for WalkSampleMapping<Node, T>
where
    T: Default,
{
    fn map_keys(&self, sample_key: SamplingKey, walk_key: Node) {
        self.inflight.insert(sample_key, T::default());
        self.inflight_reverse.insert(walk_key, sample_key);
    }
}

impl<T> SampleTrigger<(Node, Option<WrappedPath>)> for WalkSampleMapping<Node, T>
where
    T: Default,
{
    fn map_keys(&self, sample_key: SamplingKey, walk_key: (Node, Option<WrappedPath>)) {
        self.inflight.insert(sample_key, T::default());
        self.inflight_reverse.insert(walk_key.0, sample_key);
    }
}

impl<T> SampleTrigger<(Node, Option<WrappedPath>)>
    for WalkSampleMapping<(Node, Option<WrappedPath>), T>
where
    T: Default,
{
    fn map_keys(&self, sample_key: SamplingKey, walk_key: (Node, Option<WrappedPath>)) {
        self.inflight.insert(sample_key, T::default());
        self.inflight_reverse.insert(walk_key, sample_key);
    }
}

impl<S, T> WalkSampleMapping<S, T>
where
    S: Eq + fmt::Debug + hash::Hash,
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

    // Needs to be called to stop tracking the node and thus free memory.
    // Can be called from the vistor visit, or in the stream processing
    // walk output.
    pub fn complete_step(&self, s: &S) -> Option<T> {
        let reverse_mapping = self.inflight_reverse.remove(s);
        reverse_mapping
            .as_ref()
            .and_then(|(_k, sample_key)| self.inflight.remove(sample_key))
            .map(|(_k, v)| v)
    }

    pub fn is_sampling(&self, s: &S) -> bool {
        self.inflight_reverse.contains_key(s)
    }
}
