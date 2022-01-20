/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use dag::ops::DagAlgorithm;
use dag::ops::IdConvert;
use dag::ops::IdDagAlgorithm;
use dag::Id;
use dag::IdSegment;
use dag::IdSet;
use dag::IdSpan;
use dag::Set;
use dag::Vertex;
use manifest_tree::TreeStore;
use storemodel::ReadRootTreeIds;
use types::HgId;
use types::RepoPathBuf;

use crate::pathops::CompiledPaths;
use crate::pathops::ContentId;
use crate::utils;

/// State for answering history questions about paths.
pub struct PathHistory {
    // Input
    //
    /// Resolve commit ids to trees in batch.
    root_tree_reader: Arc<dyn ReadRootTreeIds + Send + Sync>,

    /// Resolve and prefetch trees in batch.
    tree_store: Arc<dyn TreeStore + Send + Sync>,

    // Derived from input
    //
    /// Commit graph algorithms. Derived from `set`.
    id_dag: Arc<dyn IdDagAlgorithm + Send + Sync>,

    /// Resolve commit ids to hashes in batch. Derived from `set`.
    id_map: Arc<dyn IdConvert + Send + Sync>,

    /// Root commits. Sorted in ASC order. Derived from `set`.
    roots: Vec<Id>,

    /// Operations to resolve content ids of `paths` starting
    /// from a root tree. Derived from `paths`.
    compiled_paths: CompiledPaths,

    // States
    //
    /// Segments to visit. They are sorted in DESC (output) order.
    queue: VecDeque<IdSegment>,

    /// Cached commit states. Include the commit hash, and content id.
    commit_map: HashMap<Id, CommitState>,

    /// Partial output. Buffer of commits with at least one path changed.
    output_buffer: VecDeque<Vertex>,

    /// Commits that are decided to be skipped.
    skipped: IdSet,

    /// Statistics for reasoning about performance.
    stats: PathHistoryStats,
}

#[derive(Debug, Default)]
pub struct PathHistoryStats {
    pub commit_count: usize,
    pub split_count: usize,
}

/// Partial information of a commit.
/// Track what's partially known.
/// Used to check paths in two commits are different.
#[derive(Debug)]
struct CommitState {
    /// Content of this commit for comparison.
    content_id: ContentId,

    /// Commit id in the IdDag.
    id: Id,

    /// Commit hash.
    vertex: Vertex,
}

impl PathHistory {
    /// Prepare context to search changes of `paths` in commit `set`.
    ///
    /// The search is lazy. Call `next()` to get the next commit.
    /// Commits are emitted in topo order, latest first.
    pub async fn new(
        set: Set,
        paths: Vec<RepoPathBuf>,
        root_tree_reader: Arc<dyn ReadRootTreeIds + Send + Sync>,
        tree_store: Arc<dyn TreeStore + Send + Sync>,
    ) -> Result<Self> {
        tracing::debug!("PathHistory::new(set={:.12?}, paths={:?})", &set, &paths);
        let (id_set, id_map) = set_to_id_set_and_id_map(&set)?;
        let (roots, id_dag) = if let Some(dag) = set.dag() {
            // "Real" roots that have no parents. That is:
            // roots(set + parents(set)) & set
            let id_dag = dag.id_dag_snapshot()?;
            let parents = id_dag.parents(id_set.clone())?;
            let union = parents.union(&id_set);
            let roots = id_dag.roots(union.clone())?;
            let roots = roots.intersection(&id_set);
            (roots, id_dag)
        } else {
            bail!("PathHistory requires Dag associated with `set`");
        };
        let roots: Vec<Id> = roots.iter_asc().collect();
        tracing::trace!(" roots: {:?}", &roots);
        let queue = id_dag.id_set_to_id_segments(&id_set)?;
        let compiled_paths = CompiledPaths::compile(paths);
        let mut path_history = Self {
            id_dag,
            id_map,
            root_tree_reader,
            tree_store,
            roots,
            compiled_paths,
            queue,
            commit_map: Default::default(),
            output_buffer: Default::default(),
            skipped: Default::default(),
            stats: Default::default(),
        };

        // Prefetch commits in segments, and roots.
        // This takes longer in a repo with lots of merges (ex. 0.2+s in linux.git).
        // In a more linear repo the overhead is smaller (ex. <0.1s).
        // PERF: It might be interesting to skip ancestors of root trees that do not
        // have the paths.
        let segments = path_history.queue.clone();
        let mut commit_ids = path_history.segments_to_prefetch_commit_ids(&mut segments.iter());
        commit_ids.extend_from_slice(&path_history.roots);
        path_history.populate_commit_states(&commit_ids).await?;

        Ok(path_history)
    }

    /// Obtain statistics for performance analysis.
    pub fn stats(&self) -> &PathHistoryStats {
        &self.stats
    }

    /// Find the next vertex that changes at least one of the specified paths.
    /// Output in DESC topo order defined by the `IdDag`.
    pub async fn next(&mut self) -> Result<Option<Vertex>> {
        loop {
            if let Some(v) = self.output_buffer.pop_front() {
                return Ok(Some(v));
            } else if self.is_done() {
                return Ok(None);
            } else {
                self.step().await?
            }
        }
    }

    /// Whether the history operation is completed.
    fn is_done(&self) -> bool {
        self.queue.is_empty()
    }

    /// Attempt to make some progress for `next()`.
    async fn step(&mut self) -> Result<()> {
        let seg = match self.queue.pop_front() {
            None => return Ok(()),
            Some(seg) => seg,
        };

        let mut action = Action::Undecided;
        let mut different_parents: Vec<Id> = Vec::new();
        let mut same_parents: Vec<Id> = Vec::new();
        let span = IdSpan::new(seg.low, seg.high);

        if self.skipped.contains(span) {
            action = Action::Skip;
        } else {
            let head_content_id = self.commit_id_to_content_id(seg.high);
            for &id in &seg.parents {
                if self.commit_id_to_content_id(id) == head_content_id {
                    same_parents.push(id)
                } else {
                    different_parents.push(id)
                }
            }

            // If there are "same parents" (content id is the same with
            // the segment head), ignore (::different_parents - ::same_parents)
            //
            //               head
            //              /    \
            //             :      :
            //   same_parent      different_parent <- segment parents
            //
            // The idea is that "same_parents" are "picked" by merges to reach
            // "head", "different_parents" are "abandoned" by the merges, and
            // not interesting.
            //
            // This is an important optimization for mergy repos.
            if action.is_undecided() && !same_parents.is_empty() {
                action = Action::OutputRoots;
                // Mark "::different_parents - ::same_parents" for skipping.
                // This also avoids showing commits that got reverted in the
                // current segment, which can cause confusion.
                if !different_parents.is_empty() {
                    let ancestors =
                        |ids: &[Id]| self.id_dag.ancestors(IdSet::from_spans(ids.to_vec()));
                    let diff_ancestors = ancestors(&different_parents)?;
                    let same_ancestors = ancestors(&same_parents)?;
                    let new_skip = diff_ancestors.difference(&same_ancestors);
                    if !new_skip.is_empty() {
                        tracing::trace!("   skip += [{:?}]", &new_skip);
                        self.skipped = self.skipped.union(&new_skip);
                    }
                }
            }

            // Single commit. Need to make final decision now.
            if action.is_undecided() && seg.low == seg.high {
                let changed = if seg.parents.is_empty() {
                    // Root. Check if it contains any of the "paths".
                    !head_content_id.is_empty()
                } else {
                    // Not a root. Check all parents are different.
                    different_parents.len() == seg.parents.len()
                };
                if changed {
                    action = Action::OutputHigh;
                } else {
                    action = Action::OutputRoots;
                }
            }
        }

        // Take action.
        tracing::debug!("  {:11?} {:?}", &action, &seg);
        match action {
            Action::Skip => {}
            Action::OutputRoots => self.output_roots(seg),
            Action::OutputHigh => self.push_output(seg.high),
            Action::Undecided => {
                self.split_segment(seg, IdSet::from_spans(different_parents))
                    .await?
            }
        }
        Ok(())
    }

    /// Output roots in the given segment.
    fn output_roots(&mut self, seg: IdSegment) {
        while let Some(&root) = self.roots.last() {
            if root < seg.low {
                break;
            }
            let _ = self.roots.pop();
            if root <= seg.high {
                let root_content_id = self.commit_id_to_content_id(root);
                if !root_content_id.is_empty() {
                    self.push_output(root);
                }
            }
        }
    }

    /// Split a segment into smaller pieces so `step()` might make progress.
    ///
    /// If a segment does not have parent (or grand parent) in
    /// `interesting_ancestors`, the segment will be skipped.
    /// `interesting_ancestors` is usually initially `different_parents`
    /// calculated in `step`.
    async fn split_segment(
        &mut self,
        seg: IdSegment,
        mut interesting_ancestors: IdSet,
    ) -> Result<()> {
        self.stats.split_count += 1;
        let mut new_segs = Vec::new();
        if seg.level > 0 {
            // Split a high level segment into lower level segments.
            let sub_segs = self.id_dag.id_segment_to_lower_level_id_segments(&seg)?;
            for seg in sub_segs.into_iter().rev() {
                if !seg.has_root
                    && seg
                        .parents
                        .iter()
                        .all(|&p| !interesting_ancestors.contains(p))
                {
                    // Split the segment. It does not contain "interesting" parents.
                    continue;
                } else {
                    interesting_ancestors =
                        interesting_ancestors.union(&IdSet::from(IdSpan::new(seg.low, seg.high)));
                    new_segs.push(seg);
                }
            }
        } else {
            // Split a flat segment into two halves.
            let mid = self.pick_mid_id(seg.low, seg.high);
            new_segs.push(IdSegment {
                high: mid,
                low: seg.low,
                parents: seg.parents,
                has_root: seg.has_root,
                level: 0,
            });
            assert!(mid < seg.high);
            new_segs.push(IdSegment {
                high: seg.high,
                low: mid + 1,
                parents: vec![mid],
                has_root: false,
                level: 0,
            });
        }

        let commit_ids = self.segments_to_prefetch_commit_ids(&mut new_segs.iter());
        self.populate_commit_states(&commit_ids).await?;
        tracing::trace!("   split to {} subsegs", new_segs.len());
        for seg in new_segs {
            self.queue.push_front(seg);
        }
        Ok(())
    }

    /// Resolve commit id to content id for easier comparison.
    /// Callsite is responsible for prefeching the commit ids.
    /// Practically, prefetching is done by code changing `self.queue`:
    /// `new` and `split_segment`.
    fn commit_id_to_content_id(&mut self, id: Id) -> ContentId {
        self.commit_map[&id].content_id
    }

    /// Given segments. Find commit ids to prefetch (high + parents).
    fn segments_to_prefetch_commit_ids(
        &mut self,
        segments: &mut dyn Iterator<Item = &IdSegment>,
    ) -> Vec<Id> {
        let mut commit_ids = Vec::new();
        for seg in segments {
            commit_ids.push(seg.high);
            commit_ids.extend_from_slice(&seg.parents);
        }
        commit_ids.sort_unstable();
        commit_ids.dedup();
        commit_ids
    }

    /// Push the commit to output. The commit is confirmed changed from its
    /// parent, or is a root with paths.
    fn push_output(&mut self, id: Id) {
        if !self.skipped.contains(id) {
            let state = &self.commit_map[&id];
            tracing::trace!("   output: {:?} ({:?})", id, &state.vertex);
            self.output_buffer.push_back(state.vertex.clone());
        }
    }

    /// Pick the bisect mid point between low and high.
    /// If low == high - 1, return low.
    fn pick_mid_id(&self, low: Id, high: Id) -> Id {
        let mid = utils::pick_mid(low.0, high.0).unwrap_or(low.0);
        Id(mid)
    }

    /// Insert initial commit states to `commit_map` if they do not already exist.
    async fn populate_commit_states(&mut self, ids: &[Id]) -> Result<()> {
        let missing_ids = ids
            .iter()
            .cloned()
            .filter(|i| !self.commit_map.contains_key(i))
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            for state in self.prepare_commit_states(&missing_ids).await? {
                self.stats.commit_count += 1;
                self.commit_map.insert(state.id, state);
            }
        }
        Ok(())
    }

    /// Prepare initial `CommitState`s for the given indexes.
    /// The initial state includes commit id, hash, root tree id.
    /// Root trees are prefetched.
    async fn prepare_commit_states(&mut self, ids: &[Id]) -> Result<Vec<CommitState>> {
        // Index => Commit id => Commit hash (vertex)
        // (about 160 commits per millisecond)
        tracing::debug!("  prepare_commit_states for {} ids", ids.len());
        let vertexes: Vec<Vertex> = self
            .id_map
            .vertex_name_batch(ids)
            .await?
            .into_iter()
            .collect::<std::result::Result<Vec<Vertex>, _>>()?;

        // Commit hash => Root tree id
        // (about 26 commits per millisecond)
        let commit_hgids: Vec<HgId> = vertexes
            .iter()
            .map(|v| HgId::from_slice(v.as_ref()))
            .collect::<std::result::Result<Vec<HgId>, _>>()?;
        let commit_to_tree_id_map: HashMap<HgId, HgId> = self
            .root_tree_reader
            .read_root_tree_ids(commit_hgids.clone())
            .await?
            .into_iter()
            .collect();
        let root_tree_ids: Vec<HgId> = commit_hgids
            .iter()
            .map(|i| commit_to_tree_id_map[i])
            .collect();

        // Root tree id => Content Id.
        // (about 14 commits per millisecond, or slower for deeper paths)
        let content_ids = self.resolve_root_tree_to_content_id(root_tree_ids).await?;

        // Construct the commit states
        let states = ids
            .iter()
            .zip(vertexes)
            .zip(content_ids)
            .map(|((id, vertex), content_id)| CommitState {
                id: *id,
                vertex,
                content_id,
            })
            .collect();

        Ok(states)
    }

    /// Resolve trees recursiely. Turn them into `ContentId` for easier comparison.
    async fn resolve_root_tree_to_content_id(
        &mut self,
        root_tree_ids: Vec<HgId>,
    ) -> Result<Vec<ContentId>> {
        let tree_store = self.tree_store.clone();
        self.compiled_paths.execute(root_tree_ids, tree_store).await
    }
}

/// Used by `step`.
#[derive(Debug, Copy, Clone)]
enum Action {
    /// Not yet decided. Needs to split the segment for better decision.
    Undecided,
    /// Skip the segment. Output nothing in the segment (not even roots).
    Skip,
    /// Skip the segment. Output roots in the segment.
    OutputRoots,
    /// Skip the segment. Output `high`. The segment length should be 1.
    OutputHigh,
}

impl Action {
    fn is_undecided(&self) -> bool {
        match self {
            Action::Undecided => true,
            _ => false,
        }
    }
}

fn set_to_id_set_and_id_map(set: &Set) -> Result<(IdSet, Arc<dyn IdConvert + Send + Sync>)> {
    match set.to_id_set_and_id_map_in_o1() {
        Some(v) => Ok(v),
        None => bail!(
            "PathHistory requires {:?} to convert to IdSet and IdMap in O(1)",
            set
        ),
    }
}
