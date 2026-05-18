/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;

use im::HashMap as ImMap;
use im::Vector as ImVec;
use smallvec::SmallVec;

use crate::SmallRevs;
use crate::linelog::PerfStats;
use crate::linelog::Rev;

/// Minimal dag implementation dedicated for linelog use-case.
/// - Only suitable for small revisions, like, 0 to 50.
/// - Assuming the total dag edges (insert calls) are small.
///   There could be O(edges * edges) complexity, and O(revs) working memory.
/// - Existing rev's parents are mutable, can be inserted after introducing rev.
///   This is different from lib/dag.
/// - Main state (parents) uses immutable data structure.
///   This fits the main `linelog`'s cheap clone design.
#[derive(Default, Clone)]
pub struct NanoDag {
    /// `parents[rev]` is the parents of `rev`.
    /// Parent revs must be smaller than `rev`.
    /// Parents are ordered (not SmallRevs).
    /// SmallVec (24 bytes) is smaller than ImVec (64 bytes).
    pub(crate) parents: ImVec<SmallVec<[Rev; 1]>>,
    /// `children` is automatically updated when `parents` is updated.
    pub(crate) children: ImMap<Rev, SmallRevs>,
    cache: OnceLock<Arc<Vec<CacheRevs>>>,
    perf_stats: Option<Arc<PerfStats>>,
}

/// Parents and other dag info associated with rev.
#[derive(Default, Clone, Debug)]
struct CacheRevs {
    ancestors: OnceLock<SmallRevs>,
    descendants: OnceLock<SmallRevs>,
}

#[derive(Clone, Copy)]
enum WalkKind {
    Ancestors,
    Descendants,
}

impl CacheRevs {
    fn revs_for_kind(&self, kind: WalkKind) -> &OnceLock<SmallRevs> {
        match kind {
            WalkKind::Ancestors => &self.ancestors,
            WalkKind::Descendants => &self.descendants,
        }
    }
}

impl NanoDag {
    /// Iterates over (rev, rev's parents) in this dag.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (Rev, &[Rev])> {
        self.parents.iter().map(AsRef::as_ref).enumerate()
    }

    /// Length of the dag. Maximum `rev` is `len() - 1`.
    pub fn len(&self) -> usize {
        self.parents.len()
    }

    /// Get the parent revs of `rev`. Returns `None` if out of bound.
    /// Parent revs preserve insertion order.
    pub fn parents(&self, rev: Rev) -> Option<&[Rev]> {
        self.parents.get(rev).map(|v| v.as_ref())
    }

    /// Get the immediate children of rev.
    /// Returns None if `rev` is unknown or has no children.
    pub fn children(&self, rev: Rev) -> Option<&SmallRevs> {
        self.children.get(&rev)
    }

    /// Get all revs present in the dag.
    pub fn all(&self) -> SmallRevs {
        SmallRevs::from_range(0..self.parents.len())
    }

    /// Revs that has no children. O(revs) to O(revs * revs).
    pub fn heads(&self, revs: &SmallRevs) -> SmallRevs {
        revs.iter()
            .filter(|rev| match self.children(*rev).cloned() {
                Some(mut children_revs) => {
                    children_revs.intersect_with(revs);
                    children_revs.is_empty()
                }
                None => true,
            })
            .collect()
    }

    /// Revs that has no parents. O(revs) to O(revs * revs).
    pub fn roots(&self, revs: &SmallRevs) -> SmallRevs {
        revs.iter()
            .filter(|rev| match self.parents(*rev) {
                Some(mut parents) => {
                    let mut parent_revs = SmallRevs::from_iter(parents.iter().copied());
                    parent_revs.intersect_with(revs);
                    parent_revs.is_empty()
                }
                None => true,
            })
            .collect()
    }

    /// Get the ancestor revs of `rev`, including `rev`.
    pub fn ancestors(&self, rev: Rev) -> Option<&SmallRevs> {
        self.walk_revs(rev, WalkKind::Ancestors)
    }

    /// Get the descendant revs of `rev`, including `rev`.
    pub fn descendants(&self, rev: Rev) -> Option<&SmallRevs> {
        self.walk_revs(rev, WalkKind::Descendants)
    }

    fn walk_revs(&self, rev: Rev, kind: WalkKind) -> Option<&SmallRevs> {
        if rev >= self.parents.len() {
            return None;
        }
        let cache = self.cache();
        if let Some(revs) = cache.get(rev)?.revs_for_kind(kind).get() {
            return Some(revs);
        }

        let mut to_visit = vec![rev];
        while let Some(visit_rev) = to_visit.pop() {
            if cache.get(visit_rev)?.revs_for_kind(kind).get().is_some() {
                continue;
            }

            let mut visit_revs = SmallRevs::from(visit_rev);
            let mut revisit = false;
            {
                let mut visit_related_rev = |related_rev: Rev| -> Option<()> {
                    match cache.get(related_rev)?.revs_for_kind(kind).get() {
                        None => {
                            if !revisit {
                                to_visit.push(visit_rev);
                                revisit = true;
                            }
                            to_visit.push(related_rev);
                        }
                        Some(related_revs) => {
                            visit_revs.union_with(related_revs);
                        }
                    }
                    Some(())
                };

                match kind {
                    WalkKind::Ancestors => {
                        for parent_rev in self.parents.get(visit_rev)? {
                            visit_related_rev(*parent_rev)?;
                        }
                    }
                    WalkKind::Descendants => {
                        if let Some(children) = self.children(visit_rev) {
                            for child_rev in children.iter() {
                                visit_related_rev(child_rev)?;
                            }
                        }
                    }
                }
            }

            if !revisit {
                cache
                    .get(visit_rev)?
                    .revs_for_kind(kind)
                    .get_or_init(|| visit_revs);
            }
        }

        cache.get(rev)?.revs_for_kind(kind).get()
    }

    /// Test if `ancestor` is an ancestor of `descendant`.
    /// `is_ancestor(rev, rev)` returns `true`.
    pub fn is_ancestor(&self, ancestor: Rev, descendant: Rev) -> bool {
        if ancestor > descendant || descendant >= self.parents.len() {
            // not topo order, or out-of-bound
            return false;
        }
        if let Some(parents) = self.parents.get(descendant) {
            // try to answer without using cache (extra allocation)
            if parents.contains(&ancestor) {
                return true;
            } else if ancestor + 1 == descendant {
                return false;
            }
        }
        let Some(ancestors) = self.ancestors(descendant) else {
            return false;
        };
        ancestors.contains(ancestor)
    }

    /// Insert a child-parent edge to the dag.
    /// If child == parent, ensure `child` rev is present in dag.
    /// Panics if parent > child.
    pub fn with_edge(self, parent: Rev, child: Rev) -> Self {
        if child == parent && child < self.parents.len() {
            return self;
        }
        assert!(child >= parent);
        let new_parents_item = match self.parents.get(child) {
            Some(parents) if parents.contains(&parent) || parent == child => {
                return self;
            }
            Some(parents) => {
                let mut parents = parents.clone();
                if parent < child {
                    parents.push(parent);
                }
                parents
            }
            None => {
                let mut parents = SmallVec::new();
                if parent < child {
                    parents.push(parent);
                }
                parents
            }
        };
        let mut new_parents = self.parents.clone();
        if new_parents.len() <= child {
            while new_parents.len() < child {
                new_parents.push_back(Default::default());
            }
            new_parents.push_back(new_parents_item);
        } else {
            new_parents.set(child, new_parents_item);
        }
        let mut new_children = self.children;
        if parent < child {
            match new_children.get(&parent) {
                None => {
                    let new_revs = SmallRevs::from(child);
                    new_children.insert(parent, new_revs);
                }
                Some(revs) => {
                    if !revs.contains(child) {
                        let mut new_revs = revs.clone();
                        new_revs.insert(child);
                        new_children.insert(parent, new_revs);
                    }
                }
            }
        }
        Self {
            parents: new_parents,
            children: new_children,
            // TRACEOFF: this invalidates all caches, but some caches can be
            // incrementally reusable. But reusing cache itself has cost...
            // anyway, hope the insert, query, insert, query (interleaved use)
            // doesn't happen often.
            cache: Default::default(),
            ..self
        }
    }

    /// Prepare the self.cache field on demand.
    fn cache(&self) -> &[CacheRevs] {
        let len = self.parents.len();
        let cache = self.cache.get_or_init(|| {
            let mut vec = Vec::with_capacity(len);
            vec.resize_with(len, Default::default);
            if let Some(stats) = &self.perf_stats {
                stats.dag_cache.fetch_add(1, Ordering::Release);
            }
            Arc::new(vec)
        });
        debug_assert_eq!(cache.len(), len);
        cache
    }

    /// Attach a `CacheStats` struct to analyse cache statistics.
    pub(crate) fn with_perf_stats(self, stats: Option<Arc<PerfStats>>) -> Self {
        Self {
            perf_stats: stats,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;

    use super::*;

    fn revs_vec(revs: &SmallRevs) -> Vec<Rev> {
        revs.iter().collect()
    }

    impl NanoDag {
        pub(crate) fn from_edges(rev_count: Rev, edges: &[(Rev, Rev)]) -> Self {
            let mut dag = NanoDag::default();
            if let Some(rev) = rev_count.checked_sub(1) {
                dag = dag.with_edge(rev, rev);
            }
            dag = edges
                .iter()
                .fold(dag, |dag, &(parent, child)| dag.with_edge(parent, child));
            for (parent, child) in edges {
                if parent >= child {
                    continue;
                }
                assert!(dag.parents(*child).unwrap().contains(parent));
                assert!(dag.children(*parent).unwrap().contains(*child));
            }
            dag
        }
    }

    fn transitive_closure(rev_count: Rev, edges: &[(Rev, Rev)]) -> Vec<SmallRevs> {
        let mut reachable: Vec<SmallRevs> = (0..rev_count).map(Into::into).collect();
        edges.iter().for_each(|&(p, c)| reachable[p].insert(c));

        for mid in 0..rev_count {
            for parent in 0..rev_count {
                if reachable[parent].contains(mid) {
                    let mid_reachable = reachable[mid].clone();
                    reachable[parent].union_with(&mid_reachable);
                }
            }
        }
        reachable
    }

    #[test]
    fn test_empty_dag_queries_are_out_of_bound() {
        let dag = NanoDag::default();

        assert_eq!(dag.parents(0), None);
        assert_eq!(dag.children(0), None);
        assert_eq!(dag.ancestors(0), None);
        assert_eq!(dag.descendants(0), None);
        assert_eq!(revs_vec(&dag.heads(&dag.all())), vec![]);
        assert_eq!(revs_vec(&dag.roots(&dag.all())), vec![]);
        assert!(!dag.is_ancestor(0, 0));
    }

    #[test]
    fn test_merge_ancestors_and_descendants() {
        // 0-{1-3,2-4}-5
        let dag = NanoDag::from_edges(6, &[(0, 1), (0, 2), (1, 3), (2, 4), (3, 5), (4, 5)]);

        assert_eq!(dag.ancestors(5).map(revs_vec), Some(vec![0, 1, 2, 3, 4, 5]));
        assert_eq!(
            dag.descendants(0).map(revs_vec),
            Some(vec![0, 1, 2, 3, 4, 5])
        );
        assert_eq!(dag.descendants(1).map(revs_vec), Some(vec![1, 3, 5]));
        assert_eq!(dag.descendants(2).map(revs_vec), Some(vec![2, 4, 5]));
        assert_eq!(dag.children(0).map(revs_vec), Some(vec![1, 2]));
        assert_eq!(dag.children(1).map(revs_vec), Some(vec![3]));
        assert_eq!(dag.children(2).map(revs_vec), Some(vec![4]));
        assert_eq!(dag.children(3).map(revs_vec), Some(vec![5]));
        assert_eq!(dag.children(4).map(revs_vec), Some(vec![5]));
        assert_eq!(dag.children(5), None);
        assert_eq!(revs_vec(&dag.heads(&dag.all())), vec![5]);
        assert_eq!(revs_vec(&dag.roots(&dag.all())), vec![0]);

        assert!(dag.is_ancestor(1, 5));
        assert!(dag.is_ancestor(2, 5));
        assert!(!dag.is_ancestor(1, 4));
        assert!(!dag.is_ancestor(2, 3));
    }

    #[test]
    fn test_heads_and_roots_are_scoped_to_revs() {
        // 0-{1-3,2-4}-5
        let dag = NanoDag::from_edges(6, &[(0, 1), (0, 2), (1, 3), (2, 4), (3, 5), (4, 5)]);

        let revs: SmallRevs = [1, 2, 3, 5].into_iter().collect();

        assert_eq!(revs_vec(&dag.heads(&revs)), vec![2, 5]);
        assert_eq!(revs_vec(&dag.roots(&revs)), vec![1, 2]);
    }

    #[test]
    fn test_is_ancestor_fast_paths_do_not_populate_cache() {
        let dag = NanoDag::from_edges(3, &[(0, 2)]);

        assert!(dag.cache.get().is_none());
        assert!(dag.is_ancestor(0, 2));
        assert!(!dag.is_ancestor(1, 2));
        assert!(!dag.is_ancestor(2, 1));
        assert!(!dag.is_ancestor(0, 3));
        assert!(dag.cache.get().is_none());
    }

    #[test]
    fn test_self_edge_resizes_dag_without_adding_parent() {
        let dag = NanoDag::default().with_edge(3, 3);

        assert_eq!(dag.parents(0), Some([].as_slice()));
        assert_eq!(dag.parents(1), Some([].as_slice()));
        assert_eq!(dag.parents(2), Some([].as_slice()));
        assert_eq!(dag.parents(3), Some([].as_slice()));
        assert_eq!(dag.children(0), None);
        assert_eq!(dag.children(3), None);
        assert_eq!(revs_vec(&dag.heads(&dag.all())), vec![0, 1, 2, 3]);
        assert_eq!(revs_vec(&dag.roots(&dag.all())), vec![0, 1, 2, 3]);
        assert_eq!(dag.ancestors(3).map(revs_vec), Some(vec![3]));
        assert_eq!(dag.descendants(3).map(revs_vec), Some(vec![3]));
        assert!(dag.is_ancestor(3, 3));
    }

    #[test]
    fn test_sparse_child_slot_and_multiple_parents() {
        let dag = NanoDag::from_edges(4, &[(1, 3), (2, 3)]);

        assert_eq!(dag.parents(0), Some([].as_slice()));
        assert_eq!(dag.parents(1), Some([].as_slice()));
        assert_eq!(dag.parents(2), Some([].as_slice()));
        assert_eq!(dag.parents(3), Some([1, 2].as_slice()));
        assert_eq!(dag.children(0), None);
        assert_eq!(dag.children(1).map(revs_vec), Some(vec![3]));
        assert_eq!(dag.children(2).map(revs_vec), Some(vec![3]));
        assert_eq!(dag.children(3), None);
        assert_eq!(revs_vec(&dag.heads(&dag.all())), vec![0, 3]);
        assert_eq!(revs_vec(&dag.roots(&dag.all())), vec![0, 1, 2]);
        assert_eq!(dag.ancestors(3).map(revs_vec), Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_duplicate_edge_preserves_existing_cache() {
        let dag = NanoDag::from_edges(3, &[(0, 1), (1, 2)]);
        assert_eq!(dag.descendants(0).map(revs_vec), Some(vec![0, 1, 2]));

        let duplicated = dag.clone().with_edge(0, 1);
        assert_eq!(duplicated.parents(1), Some([0].as_slice()));
        assert_eq!(duplicated.children(0).map(revs_vec), Some(vec![1]));
        assert_eq!(duplicated.descendants(0).map(revs_vec), Some(vec![0, 1, 2]));
        assert!(Arc::ptr_eq(
            dag.cache.get().expect("cache should be populated"),
            duplicated
                .cache
                .get()
                .expect("duplicate edge should preserve cache"),
        ));
    }

    #[test]
    fn test_with_edge_invalidates_derived_cache_without_mutating_original() {
        let dag = NanoDag::from_edges(3, &[(0, 1), (1, 2)]);
        assert_eq!(dag.descendants(0).map(revs_vec), Some(vec![0, 1, 2]));

        let extended = dag.clone().with_edge(2, 3);
        assert_eq!(dag.descendants(0).map(revs_vec), Some(vec![0, 1, 2]));
        assert_eq!(dag.children(2), None);
        assert_eq!(
            extended.descendants(0).map(revs_vec),
            Some(vec![0, 1, 2, 3])
        );
        assert_eq!(extended.ancestors(3).map(revs_vec), Some(vec![0, 1, 2, 3]));
        assert_eq!(extended.children(2).map(revs_vec), Some(vec![3]));
    }

    #[test]
    #[should_panic]
    fn test_with_edge_panics_if_parent_is_after_child() {
        let _ = NanoDag::default().with_edge(2, 1);
    }

    quickcheck! {
        fn check_reachability_matches_transitive_closure(edges: Vec<(u8, u8)>) -> bool {
            const REV_COUNT: Rev = 16;
            const EDGE_LIMIT: usize = 64;

            let edges: Vec<_> = edges
                .into_iter()
                .take(EDGE_LIMIT)
                .map(|(a, b)| {
                    let a = a as Rev % REV_COUNT;
                    let b = b as Rev % REV_COUNT;
                    (a.min(b), a.max(b))
                })
                .collect();
            let dag = NanoDag::from_edges(REV_COUNT, &edges);
            let reachable = transitive_closure(REV_COUNT, &edges);

            (0..REV_COUNT).all(|ancestor| {
                (0..REV_COUNT).all(|descendant| {
                    let expected = reachable[ancestor].contains(descendant);
                    dag.is_ancestor(ancestor, descendant) == expected
                        && dag
                            .ancestors(descendant)
                            .is_some_and(|revs| revs.contains(ancestor) == expected)
                        && dag
                            .descendants(ancestor)
                            .is_some_and(|revs| revs.contains(descendant) == expected)
                })
            })
        }
    }
}
