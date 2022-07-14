/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::detail::graph::EdgeType;
use crate::detail::graph::Node;
use crate::detail::graph::NodeData;
use crate::detail::graph::NodeType;
use crate::detail::graph::WrappedPathHash;
use crate::detail::graph::WrappedPathLike;
use crate::detail::state::InternedType;
use crate::detail::state::StepStats;
use crate::detail::state::WalkState;
use crate::detail::walk::EmptyRoute;
use crate::detail::walk::OutgoingEdge;
use crate::detail::walk::StepRoute;
use crate::detail::walk::TailingWalkVisitor;
use crate::detail::walk::VisitOne;
use crate::detail::walk::WalkVisitor;

use anyhow::Error;
use async_trait::async_trait;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use bulkops::Direction;
use context::CoreContext;
use context::SamplingKey;
use dashmap::DashMap;
use mercurial_types::HgChangesetId;
use mononoke_types::datetime::DateTime;
use mononoke_types::ChangesetId;
use phases::Phases;
use regex::Regex;
use slog::Logger;
use std::collections::HashSet;
use std::fmt;
use std::hash;
use std::sync::Arc;

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
        chunk_direction: Option<Direction>,
    ) -> Self {
        Self {
            inner: WalkState::new(
                include_node_types,
                include_edge_types,
                HashSet::new(),
                enable_derive,
                chunk_direction,
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
    fn get_hg_from_bonsai(&self, bcs_id: &ChangesetId) -> Option<HgChangesetId> {
        self.inner.get_hg_from_bonsai(bcs_id)
    }
    fn record_hg_from_bonsai(&self, bcs_id: &ChangesetId, hg_cs_id: HgChangesetId) {
        self.inner.record_hg_from_bonsai(bcs_id, hg_cs_id)
    }
    async fn get_bonsai_from_hg(
        &self,
        ctx: &CoreContext,
        bonsai_hg_mapping: &dyn BonsaiHgMapping,
        hg_cs_id: &HgChangesetId,
    ) -> Result<ChangesetId, Error> {
        self.inner
            .get_bonsai_from_hg(ctx, bonsai_hg_mapping, hg_cs_id)
            .await
    }
    async fn defer_from_hg(
        &self,
        ctx: &CoreContext,
        bonsai_hg_mapping: &dyn BonsaiHgMapping,
        hg_cs_id: &HgChangesetId,
    ) -> Result<Option<ChangesetId>, Error> {
        self.inner
            .defer_from_hg(ctx, bonsai_hg_mapping, hg_cs_id)
            .await
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PathTrackingRoute<P: WrappedPathLike> {
    // The path we reached this by
    pub path: Option<P>,
    /// When did this route see this path was updated.
    /// Taken from the last bonsai or hg changset stepped through.
    pub mtime: Option<DateTime>,
}

// We don't hold these tracking so as to keep memory usage down in scrub
impl<P> StepRoute for PathTrackingRoute<P>
where
    P: WrappedPathLike + fmt::Debug,
{
    fn source_node(&self) -> Option<&Node> {
        None
    }
    fn via_node(&self) -> Option<&Node> {
        None
    }
}

impl<P> PathTrackingRoute<P>
where
    P: WrappedPathLike + Eq + Clone,
{
    fn evolve(route: Option<Self>, walk_item: &OutgoingEdge, mtime: Option<&DateTime>) -> Self {
        let existing_path = route.as_ref().and_then(|r| r.path.as_ref());
        let existing_mtime = route.as_ref().and_then(|r| r.mtime.as_ref());
        let new_path = P::evolve_path(existing_path, walk_item);

        // reuse same route if possible
        if new_path == existing_path && (mtime.is_none() || mtime == existing_mtime) {
            if let Some(route) = route {
                return route;
            }
        }

        Self {
            path: new_path.cloned(),
            mtime: mtime.cloned().or_else(|| route.and_then(|r| r.mtime)),
        }
    }
}

// Name the stream output key type
#[derive(Debug, Eq, Hash, PartialEq)]
pub struct WalkKeyOptPath<P: WrappedPathLike> {
    pub node: Node,
    pub path: Option<P>,
}

// Map the key type so progress reporting works
impl<'a, P: WrappedPathLike> From<&'a WalkKeyOptPath<P>> for &'a Node {
    fn from(from: &'a WalkKeyOptPath<P>) -> &'a Node {
        &from.node
    }
}

// Name the stream output payload type
#[derive(Default)]
pub struct WalkPayloadMtime {
    pub data: Option<NodeData>,
    pub mtime: Option<DateTime>,
}

impl<T> TailingWalkVisitor for SamplingWalkVisitor<T> {
    fn start_chunk(
        &mut self,
        chunk_members: &HashSet<ChangesetId>,
        mapping_prepop: Vec<BonsaiHgMappingEntry>,
    ) -> Result<HashSet<OutgoingEdge>, Error> {
        self.inner.start_chunk(chunk_members, mapping_prepop)
    }

    fn clear_state(
        &mut self,
        node_types: &HashSet<NodeType>,
        interned_types: &HashSet<InternedType>,
    ) {
        self.inner.clear_state(node_types, interned_types)
    }

    fn end_chunks(&mut self, logger: &Logger, contiguous_bounds: bool) -> Result<(), Error> {
        self.inner.end_chunks(logger, contiguous_bounds)
    }

    fn num_deferred(&self) -> usize {
        self.inner.num_deferred()
    }
}

impl<T, P>
    WalkVisitor<(WalkKeyOptPath<P>, WalkPayloadMtime, Option<StepStats>), PathTrackingRoute<P>>
    for SamplingWalkVisitor<T>
where
    T: SampleTrigger<WalkKeyOptPath<P>> + Send + Sync,
    P: WrappedPathLike + Clone + Eq + fmt::Display,
{
    fn start_step(
        &self,
        mut ctx: CoreContext,
        route: Option<&PathTrackingRoute<P>>,
        step: &OutgoingEdge,
    ) -> Option<CoreContext> {
        if self.options.node_types.contains(&step.target.get_type()) {
            let repo_path = route.and_then(|r| P::evolve_path(r.path.as_ref(), step));
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
                            |r| Some(r.sampling_fingerprint()),
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
                        WalkKeyOptPath {
                            node: step.target.clone(),
                            path: repo_path.cloned(),
                        },
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
        route: Option<PathTrackingRoute<P>>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (
        (WalkKeyOptPath<P>, WalkPayloadMtime, Option<StepStats>),
        PathTrackingRoute<P>,
        Vec<OutgoingEdge>,
    ) {
        let inner_route = route.as_ref().map(|_| EmptyRoute {});

        let mtime = match &node_data {
            Some(NodeData::Changeset(bcs)) => {
                bcs.committer_date().or_else(|| Some(bcs.author_date()))
            }
            Some(NodeData::HgChangeset(hg_cs)) => Some(hg_cs.time()),
            _ => None,
        };

        let route = PathTrackingRoute::evolve(route, &resolved, mtime);
        let ((n, nd, stats), _inner_route, outgoing) =
            self.inner
                .visit(ctx, resolved, node_data, inner_route, outgoing);

        (
            (
                WalkKeyOptPath {
                    node: n,
                    path: route.path.clone(),
                },
                WalkPayloadMtime {
                    data: nd,
                    mtime: route.mtime.clone(),
                },
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
        route: Option<PathTrackingRoute<P>>,
    ) -> Result<
        (
            (WalkKeyOptPath<P>, WalkPayloadMtime, Option<StepStats>),
            PathTrackingRoute<P>,
        ),
        Error,
    > {
        let inner_route = route.as_ref().map(|_| EmptyRoute {});
        let route = PathTrackingRoute::evolve(route, walk_item, None);
        let ((n, _nd, stats), _inner_route) =
            self.inner.defer_visit(bcs_id, walk_item, inner_route)?;
        Ok((
            (
                WalkKeyOptPath {
                    node: n,
                    path: route.path.clone(),
                },
                WalkPayloadMtime::default(),
                stats,
            ),
            route,
        ))
    }
}

// Super simple sampling visitor impl for scrubbing
impl<T>
    WalkVisitor<
        (
            WalkKeyOptPath<WrappedPathHash>,
            WalkPayloadMtime,
            Option<StepStats>,
        ),
        EmptyRoute,
    > for SamplingWalkVisitor<T>
where
    T: SampleTrigger<WalkKeyOptPath<WrappedPathHash>> + Send + Sync,
{
    fn start_step(
        &self,
        mut ctx: CoreContext,
        route: Option<&EmptyRoute>,
        step: &OutgoingEdge,
    ) -> Option<CoreContext> {
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
                self.sampler.map_keys(
                    sampling_key,
                    WalkKeyOptPath {
                        node: step.target.clone(),
                        path: None,
                    },
                );
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
        (
            WalkKeyOptPath<WrappedPathHash>,
            WalkPayloadMtime,
            Option<StepStats>,
        ),
        EmptyRoute,
        Vec<OutgoingEdge>,
    ) {
        let ((n, nd, stats), route, outgoing) =
            self.inner.visit(ctx, resolved, node_data, route, outgoing);
        let output = (
            WalkKeyOptPath {
                node: n,
                path: None,
            },
            WalkPayloadMtime {
                data: nd,
                mtime: None,
            },
            stats,
        );
        (output, route, outgoing)
    }

    fn defer_visit(
        &self,
        bcs_id: &ChangesetId,
        walk_item: &OutgoingEdge,
        route: Option<EmptyRoute>,
    ) -> Result<
        (
            (
                WalkKeyOptPath<WrappedPathHash>,
                WalkPayloadMtime,
                Option<StepStats>,
            ),
            EmptyRoute,
        ),
        Error,
    > {
        let ((n, nd, stats), route) = self.inner.defer_visit(bcs_id, walk_item, route)?;
        let output = (
            WalkKeyOptPath {
                node: n,
                path: None,
            },
            WalkPayloadMtime {
                data: nd,
                mtime: None,
            },
            stats,
        );
        Ok((output, route))
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

impl<T, P> SampleTrigger<WalkKeyOptPath<P>> for WalkSampleMapping<Node, T>
where
    T: Default,
    P: WrappedPathLike + Eq + hash::Hash + fmt::Debug,
{
    fn map_keys(&self, sample_key: SamplingKey, walk_key: WalkKeyOptPath<P>) {
        self.inflight.insert(sample_key, T::default());
        self.inflight_reverse.insert(walk_key.node, sample_key);
    }
}

impl<T, P> SampleTrigger<WalkKeyOptPath<P>> for WalkSampleMapping<WalkKeyOptPath<P>, T>
where
    T: Default,
    P: WrappedPathLike + Eq + hash::Hash + fmt::Debug,
{
    fn map_keys(&self, sample_key: SamplingKey, walk_key: WalkKeyOptPath<P>) {
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
