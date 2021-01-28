/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{EdgeType, Node, NodeData, NodeType, WrappedPath};
use crate::state::{InternedType, StepStats, WalkState};
use crate::walk::{EmptyRoute, OutgoingEdge, StepRoute, TailingWalkVisitor, VisitOne, WalkVisitor};

use anyhow::Error;
use async_trait::async_trait;
use bonsai_hg_mapping::BonsaiHgMapping;
use context::{CoreContext, SamplingKey};
use dashmap::DashMap;
use mercurial_types::HgChangesetId;
use mononoke_types::{datetime::DateTime, ChangesetId, RepositoryId};
use phases::Phases;
use regex::Regex;
use std::{collections::HashSet, fmt, hash, sync::Arc};

pub trait SampleTrigger<K> {
    fn map_keys(&self, key: SamplingKey, walk_key: K);
}

#[derive(Clone, Debug, Default)]
pub struct SamplingOptions {
    pub sample_rate: u64,
    pub sample_offset: u64,
    pub node_types: HashSet<NodeType>,
    pub exclude_types: HashSet<NodeType>,
}

impl SamplingOptions {
    pub fn retain_or_default(&mut self, walk_include: &HashSet<NodeType>) {
        if self.node_types.is_empty() {
            self.node_types = walk_include
                .iter()
                .filter(|e| !self.exclude_types.contains(e))
                .cloned()
                .collect();
        } else {
            self.node_types.retain(|i| walk_include.contains(i));
        }
    }
}

pub struct SamplingWalkVisitor<T> {
    inner: WalkState,
    options: SamplingOptions,
    sample_path_regex: Option<Regex>,
    sampler: Arc<T>,
}

impl<T> SamplingWalkVisitor<T> {
    pub fn new(
        include_node_types: HashSet<NodeType>,
        include_edge_types: HashSet<EdgeType>,
        options: SamplingOptions,
        sample_path_regex: Option<Regex>,
        sampler: Arc<T>,
        enable_derive: bool,
    ) -> Self {
        Self {
            inner: WalkState::new(
                include_node_types,
                include_edge_types,
                HashSet::new(),
                enable_derive,
            ),
            options,
            sample_path_regex,
            sampler,
        }
    }
}

#[async_trait]
impl<T: Send + Sync> VisitOne for SamplingWalkVisitor<T> {
    fn in_chunk(&self, bcs_id: &ChangesetId) -> bool {
        self.inner.in_chunk(bcs_id)
    }
    fn needs_visit(&self, outgoing: &OutgoingEdge) -> bool {
        self.inner.needs_visit(outgoing)
    }
    async fn is_public(
        &self,
        ctx: &CoreContext,
        phases_store: &dyn Phases,
        bcs_id: &ChangesetId,
    ) -> Result<bool, Error> {
        self.inner.is_public(ctx, phases_store, bcs_id).await
    }
    async fn defer_from_hg(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        bonsai_hg_mapping: &dyn BonsaiHgMapping,
        hg_cs_id: &HgChangesetId,
    ) -> Result<Option<ChangesetId>, Error> {
        self.inner
            .defer_from_hg(ctx, repo_id, bonsai_hg_mapping, hg_cs_id)
            .await
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

// No useful node info held.  TODO(ahornby) be nice to expand StepRoute logging to show path if present
impl StepRoute for PathTrackingRoute {
    fn source_node(&self) -> Option<&Node> {
        None
    }
    fn via_node(&self) -> Option<&Node> {
        None
    }
}

// Only certain node types can have repo paths associated
fn filter_repo_path(node_type: NodeType, path: Option<&'_ WrappedPath>) -> Option<&'_ WrappedPath> {
    match node_type {
        NodeType::Root => None,
        // Bonsai
        NodeType::Bookmark => None,
        NodeType::Changeset => None,
        NodeType::BonsaiHgMapping => None,
        NodeType::PhaseMapping => None,
        NodeType::PublishedBookmarks => None,
        // Hg
        NodeType::HgBonsaiMapping => None,
        NodeType::HgChangeset => None,
        NodeType::HgChangesetViaBonsai => None,
        NodeType::HgManifest => path,
        NodeType::HgFileEnvelope => path,
        NodeType::HgFileNode => path,
        // Content
        NodeType::FileContent => path,
        NodeType::FileContentMetadata => path,
        NodeType::AliasContentMapping => path,
        // Derived Data
        NodeType::Blame => None,
        NodeType::ChangesetInfo => None,
        NodeType::ChangesetInfoMapping => None,
        NodeType::DeletedManifest => path,
        NodeType::DeletedManifestMapping => None,
        NodeType::FastlogBatch => path,
        NodeType::FastlogDir => path,
        NodeType::FastlogFile => path,
        NodeType::Fsnode => path,
        NodeType::FsnodeMapping => None,
        NodeType::SkeletonManifest => path,
        NodeType::SkeletonManifestMapping => None,
        NodeType::UnodeFile => path,
        NodeType::UnodeManifest => path,
        NodeType::UnodeMapping => None,
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

// Name the stream output key type
#[derive(Debug, Eq, Hash, PartialEq)]
pub struct WalkKeyOptPath(pub Node, pub Option<WrappedPath>);

// Map the key type so progress reporting works
impl<'a> From<&'a WalkKeyOptPath> for &'a Node {
    fn from(WalkKeyOptPath(n, _p): &'a WalkKeyOptPath) -> &'a Node {
        n
    }
}

// Name the stream output payload type
pub struct WalkPayloadMtime(pub Option<DateTime>, pub Option<NodeData>);

impl<T> TailingWalkVisitor for SamplingWalkVisitor<T> {
    fn start_chunk(
        &mut self,
        chunk_members: &HashSet<ChangesetId>,
    ) -> Result<HashSet<OutgoingEdge>, Error> {
        self.inner.start_chunk(chunk_members)
    }

    fn clear_state(
        &mut self,
        node_types: &HashSet<NodeType>,
        interned_types: &HashSet<InternedType>,
    ) {
        self.inner.clear_state(node_types, interned_types)
    }

    fn end_chunks(&mut self) -> Result<(), Error> {
        self.inner.end_chunks()
    }

    fn num_deferred(&self) -> usize {
        self.inner.num_deferred()
    }
}

impl<T> WalkVisitor<(WalkKeyOptPath, WalkPayloadMtime, Option<StepStats>), PathTrackingRoute>
    for SamplingWalkVisitor<T>
where
    T: SampleTrigger<WalkKeyOptPath> + Send + Sync,
{
    fn start_step(
        &self,
        mut ctx: CoreContext,
        route: Option<&PathTrackingRoute>,
        step: &OutgoingEdge,
    ) -> CoreContext {
        if self.options.node_types.contains(&step.target.get_type()) {
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
                let should_sample = match self.options.sample_rate {
                    0 => false,
                    1 => true,
                    sample_rate => {
                        let sampling_fingerprint = repo_path.map_or_else(
                            || step.target.sampling_fingerprint(),
                            |r| r.sampling_fingerprint(),
                        );
                        sampling_fingerprint
                            .map_or(self.options.sample_offset % sample_rate == 0, |fp| {
                                (fp + self.options.sample_offset) % sample_rate == 0
                            })
                    }
                };

                if should_sample {
                    let sampling_key = SamplingKey::new();
                    ctx = ctx.clone_and_sample(sampling_key);
                    self.sampler.map_keys(
                        sampling_key,
                        WalkKeyOptPath(step.target.clone(), repo_path.cloned()),
                    );
                }
            }
        }
        self.inner
            .start_step(ctx, route.map(|_| &EmptyRoute {}), step)
    }

    fn visit(
        &self,
        ctx: &CoreContext,
        resolved: OutgoingEdge,
        node_data: Option<NodeData>,
        route: Option<PathTrackingRoute>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (
        (WalkKeyOptPath, WalkPayloadMtime, Option<StepStats>),
        PathTrackingRoute,
        Vec<OutgoingEdge>,
    ) {
        let inner_route = route.as_ref().map(|_| EmptyRoute {});

        let mtime = match &node_data {
            Some(NodeData::Changeset(bcs)) => {
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
            (
                WalkKeyOptPath(n, route.path.clone()),
                WalkPayloadMtime(route.mtime.clone(), nd),
                stats,
            ),
            route,
            outgoing,
        )
    }

    fn defer_visit(
        &self,
        bcs_id: &ChangesetId,
        walk_item: &OutgoingEdge,
        route: Option<PathTrackingRoute>,
    ) -> (
        (WalkKeyOptPath, WalkPayloadMtime, Option<StepStats>),
        PathTrackingRoute,
    ) {
        let inner_route = route.as_ref().map(|_| EmptyRoute {});
        let route =
            PathTrackingRoute::evolve(route, walk_item.path.as_ref(), &walk_item.target, None);
        let ((n, _nd, stats), _inner_route) =
            self.inner.defer_visit(bcs_id, walk_item, inner_route);
        (
            (
                WalkKeyOptPath(n, route.path.clone()),
                WalkPayloadMtime(None, None),
                stats,
            ),
            route,
        )
    }
}

// Super simple sampling visitor impl for scrubbing
impl<T> WalkVisitor<(Node, Option<NodeData>, Option<StepStats>), EmptyRoute>
    for SamplingWalkVisitor<T>
where
    T: SampleTrigger<Node> + Send + Sync,
{
    fn start_step(
        &self,
        mut ctx: CoreContext,
        route: Option<&EmptyRoute>,
        step: &OutgoingEdge,
    ) -> CoreContext {
        if self.options.node_types.contains(&step.target.get_type()) {
            let should_sample = match self.options.sample_rate {
                0 => false,
                1 => true,
                sample_rate => step
                    .target
                    .sampling_fingerprint()
                    .map_or(self.options.sample_offset % sample_rate == 0, |fp| {
                        (fp + self.options.sample_offset) % sample_rate == 0
                    }),
            };

            if should_sample {
                let sampling_key = SamplingKey::new();
                ctx = ctx.clone_and_sample(sampling_key);
                self.sampler.map_keys(sampling_key, step.target.clone());
            }
        }
        self.inner.start_step(ctx, route, step)
    }

    fn visit(
        &self,
        ctx: &CoreContext,
        resolved: OutgoingEdge,
        node_data: Option<NodeData>,
        route: Option<EmptyRoute>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (
        (Node, Option<NodeData>, Option<StepStats>),
        EmptyRoute,
        Vec<OutgoingEdge>,
    ) {
        self.inner.visit(ctx, resolved, node_data, route, outgoing)
    }

    fn defer_visit(
        &self,
        bcs_id: &ChangesetId,
        walk_item: &OutgoingEdge,
        route: Option<EmptyRoute>,
    ) -> ((Node, Option<NodeData>, Option<StepStats>), EmptyRoute) {
        self.inner.defer_visit(bcs_id, walk_item, route)
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

impl<T> SampleTrigger<WalkKeyOptPath> for WalkSampleMapping<Node, T>
where
    T: Default,
{
    fn map_keys(&self, sample_key: SamplingKey, walk_key: WalkKeyOptPath) {
        self.inflight.insert(sample_key, T::default());
        self.inflight_reverse.insert(walk_key.0, sample_key);
    }
}

impl<T> SampleTrigger<WalkKeyOptPath> for WalkSampleMapping<WalkKeyOptPath, T>
where
    T: Default,
{
    fn map_keys(&self, sample_key: SamplingKey, walk_key: WalkKeyOptPath) {
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
