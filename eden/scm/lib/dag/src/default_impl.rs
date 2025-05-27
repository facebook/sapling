/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use futures::StreamExt;
use futures::TryStreamExt;

use crate::DagAlgorithm;
use crate::Group;
use crate::Id;
use crate::IdSet;
use crate::Result;
use crate::Set;
use crate::Vertex;
use crate::VertexListWithOptions;
use crate::dag::MemDag;
use crate::errors::programming;
use crate::ops::DagAddHeads;
use crate::ops::IdConvert;
use crate::ops::IdDagAlgorithm;
use crate::ops::Parents;
use crate::ops::ToIdSet;
use crate::ops::ToSet;
use crate::set::hints::Hints;
use crate::utils;

/// Re-create the graph so it looks better when rendered.
///
/// See `utils::beautify_graph` for details.
///
/// For example, the left-side graph will be rewritten to the right-side:
///
/// 1. Linearize.
///
/// ```plain,ignore
///   A             A      # Linearize is done by IdMap::assign_heads,
///   |             |      # as long as the heads provided are the heads
///   | C           B      # of the whole graph ("A", "C", not "B", "D").
///   | |           |
///   B |     ->    | C
///   | |           | |
///   | D           | D
///   |/            |/
///   E             E
/// ```
///
/// 2. Reorder branches (at different branching points) to reduce columns.
///
/// ```plain,ignore
///     D           B
///     |           |      # Assuming the main branch is B-C-E.
///   B |           | A    # Branching point of the D branch is "C"
///   | |           |/     # Branching point of the A branch is "C"
///   | | A   ->    C      # The D branch should be moved to below
///   | |/          |      # the A branch.
///   | |           | D
///   |/|           |/
///   C /           E
///   |/
///   E
/// ```
///
/// 3. Reorder branches (at a same branching point) to reduce length of
///    edges.
///
/// ```plain,ignore
///   D              A
///   |              |     # This is done by picking the longest
///   | A            B     # branch (A-B-C-E) as the "main branch"
///   | |            |     # and work on the remaining branches
///   | B     ->     C     # recursively.
///   | |            |
///   | C            | D
///   |/             |/
///   E              E
/// ```
///
/// `main_branch` optionally defines how to sort the heads. A head `x` will
/// be emitted first during iteration, if `x` is in `main_branch`.
///
/// This function is expensive. Only run on small graphs.
pub(crate) async fn beautify(
    this: &(impl DagAlgorithm + ?Sized),
    main_branch: Option<Set>,
) -> Result<MemDag> {
    // Prepare input for utils::beautify_graph.
    // Maintain usize <-> Vertex map. Also fetch the Vertex <-> Id mapping (via all.iter).
    let all = this.all().await?;
    let usize_to_vertex: Vec<Vertex> = all.iter_rev().await?.try_collect().await?;
    let vertex_to_usize: HashMap<Vertex, usize> = usize_to_vertex
        .iter()
        .enumerate()
        .map(|(i, v)| (v.clone(), i))
        .collect();
    let mut priorities = Vec::new();
    let main_branch = main_branch.unwrap_or_else(Set::empty);

    let mut parents_vec = Vec::with_capacity(usize_to_vertex.len());
    for (i, vertex) in usize_to_vertex.iter().enumerate() {
        if main_branch.contains(vertex).await? {
            priorities.push(i);
        }
        let parent_vertexes = this.parent_names(vertex.clone()).await?;
        let parent_usizes: Vec<usize> = parent_vertexes
            .iter()
            .filter_map(|p| vertex_to_usize.get(p))
            .copied()
            .collect();
        parents_vec.push(parent_usizes);
    }

    // Call utils::beautify_graph.
    let sorted = utils::beautify_graph(&parents_vec, &priorities);

    // Recreate the graph using the given order.
    let mut dag = MemDag::new();
    let snapshot = this.dag_snapshot()?;
    for i in sorted.into_iter().rev() {
        let heads: Vec<Vertex> = vec![usize_to_vertex[i].clone()];
        dag.add_heads(&snapshot, &heads.into()).await?;
    }
    Ok(dag)
}

/// Provide a sub-graph containing only the specified set.
pub(crate) async fn subdag(this: &(impl DagAlgorithm + ?Sized), set: Set) -> Result<MemDag> {
    let set = this.sort(&set).await?;
    let parents = match set.to_parents().await? {
        Some(p) => p,
        None => return programming("Set returned by dag.sort() should support to_parents()"),
    };
    let mut dag = MemDag::new();
    let heads = this.heads_ancestors(set).await?;
    // "heads" is in DESC order. Use reversed order for insertion so the
    // resulting subdag might preserve the same order with the original dag.
    let heads: Vec<Vertex> = heads.iter_rev().await?.try_collect().await?;
    // MASTER group enables the ONLY_HEAD segment flag. It improves graph query performance.
    let heads = VertexListWithOptions::from(heads).with_desired_group(Group::MASTER);
    dag.add_heads(&parents, &heads).await?;
    Ok(dag)
}

/// Convert `Set` to a `Parents` implementation that only returns vertexes in the set.
pub(crate) async fn set_to_parents(set: &Set) -> Result<Option<impl Parents + use<>>> {
    let (id_set, id_map) = match set.to_id_set_and_id_map_in_o1() {
        Some(v) => v,
        None => return Ok(None),
    };
    let dag = match set.dag() {
        None => return Ok(None),
        Some(dag) => dag,
    };
    let id_dag = dag.id_dag_snapshot()?;

    // Pre-resolve ids to vertexes. Reduce remote lookup round-trips.
    let ids: Vec<Id> = id_set.iter_desc().collect();
    id_map.vertex_name_batch(&ids).await?;

    struct IdParents {
        id_set: IdSet,
        id_dag: Arc<dyn IdDagAlgorithm + Send + Sync>,
        id_map: Arc<dyn IdConvert + Send + Sync>,
    }

    #[async_trait::async_trait]
    impl Parents for IdParents {
        async fn parent_names(&self, name: Vertex) -> Result<Vec<Vertex>> {
            tracing::debug!(
                target: "dag::idparents",
                "resolving parents for {:?}", &name,
            );
            let id = self.id_map.vertex_id(name).await?;
            let direct_parent_ids = self.id_dag.parent_ids(id)?;
            let parent_ids = if direct_parent_ids.iter().all(|&id| self.id_set.contains(id)) {
                // Fast path. No "leaked" parents.
                direct_parent_ids
            } else {
                // Slower path.
                // PERF: There might be room to optimize (ex. dedicated API like
                // reachable_roots).
                let parent_id_set = IdSet::from_spans(direct_parent_ids);
                let ancestors = self.id_dag.ancestors(parent_id_set)?;
                let heads = ancestors.intersection(&self.id_set);
                let heads = self.id_dag.heads_ancestors(heads)?;
                heads.iter_desc().collect()
            };

            let vertexes = self.id_map.vertex_name_batch(&parent_ids).await?;
            let parents = vertexes.into_iter().collect::<Result<Vec<_>>>()?;
            Ok(parents)
        }

        async fn hint_subdag_for_insertion(&self, _heads: &[Vertex]) -> Result<MemDag> {
            // The `IdParents` is not intended to be inserted to other graphs.
            tracing::warn!(
                target: "dag::idparents",
                "IdParents does not implement hint_subdag_for_insertion() for efficient insertion"
            );
            Ok(MemDag::new())
        }
    }

    let parents = IdParents {
        id_set,
        id_dag,
        id_map,
    };

    Ok(Some(parents))
}

pub(crate) async fn parents(this: &(impl DagAlgorithm + ?Sized), set: Set) -> Result<Set> {
    let mut result: Vec<Vertex> = Vec::new();
    let mut iter = set.iter().await?;
    // PERF: This is not an efficient async implementation.
    while let Some(vertex) = iter.next().await {
        let parents = this.parent_names(vertex?).await?;
        result.extend(parents);
    }
    Ok(Set::from_static_names(result))
}

pub(crate) async fn first_ancestor_nth(
    this: &(impl DagAlgorithm + ?Sized),
    name: Vertex,
    n: u64,
) -> Result<Option<Vertex>> {
    let mut vertex = name.clone();
    for _ in 0..n {
        let parents = this.parent_names(vertex).await?;
        if parents.is_empty() {
            return Ok(None);
        }
        vertex = parents[0].clone();
    }
    Ok(Some(vertex))
}

pub(crate) async fn first_ancestors(this: &(impl DagAlgorithm + ?Sized), set: Set) -> Result<Set> {
    let mut to_visit: Vec<Vertex> = {
        let mut list = Vec::with_capacity(set.count_slow().await?.try_into()?);
        let mut iter = set.iter().await?;
        while let Some(next) = iter.next().await {
            let vertex = next?;
            list.push(vertex);
        }
        list
    };
    let mut visited: HashSet<Vertex> = to_visit.clone().into_iter().collect();
    while let Some(v) = to_visit.pop() {
        #[allow(clippy::never_loop)]
        if let Some(parent) = this.parent_names(v).await?.into_iter().next() {
            if visited.insert(parent.clone()) {
                to_visit.push(parent);
            }
        }
    }
    let hints = Hints::new_inherit_idmap_dag(set.hints());
    let set = Set::from_iter(visited.into_iter().map(Ok), hints);
    this.sort(&set).await
}

pub(crate) async fn heads(this: &(impl DagAlgorithm + ?Sized), set: Set) -> Result<Set> {
    Ok(set.clone() - this.parents(set).await?)
}

pub(crate) async fn roots(this: &(impl DagAlgorithm + ?Sized), set: Set) -> Result<Set> {
    Ok(set.clone() - this.children(set).await?)
}

pub(crate) async fn merges(this: &(impl DagAlgorithm + ?Sized), set: Set) -> Result<Set> {
    let this = this.dag_snapshot()?;
    Ok(set.filter(Box::new(move |v: &Vertex| {
        let this = this.clone();
        Box::pin(async move {
            DagAlgorithm::parent_names(&this, v.clone())
                .await
                .map(|ps| ps.len() >= 2)
        })
    })))
}

pub(crate) async fn reachable_roots(
    this: &(impl DagAlgorithm + ?Sized),
    roots: Set,
    heads: Set,
) -> Result<Set> {
    let heads_ancestors = this.ancestors(heads.clone()).await?;
    let roots = roots & heads_ancestors.clone(); // Filter out "bogus" roots.
    let only = heads_ancestors - this.ancestors(roots.clone()).await?;
    Ok(roots.clone() & (heads.clone() | this.parents(only).await?))
}

pub(crate) async fn heads_ancestors(this: &(impl DagAlgorithm + ?Sized), set: Set) -> Result<Set> {
    this.heads(this.ancestors(set).await?).await
}

pub(crate) async fn only(
    this: &(impl DagAlgorithm + ?Sized),
    reachable: Set,
    unreachable: Set,
) -> Result<Set> {
    let reachable = this.ancestors(reachable).await?;
    let unreachable = this.ancestors(unreachable).await?;
    Ok(reachable - unreachable)
}

pub(crate) async fn only_both(
    this: &(impl DagAlgorithm + ?Sized),
    reachable: Set,
    unreachable: Set,
) -> Result<(Set, Set)> {
    let reachable = this.ancestors(reachable).await?;
    let unreachable = this.ancestors(unreachable).await?;
    Ok((reachable - unreachable.clone(), unreachable))
}

pub(crate) async fn gca_one(
    this: &(impl DagAlgorithm + ?Sized),
    set: Set,
) -> Result<Option<Vertex>> {
    this.gca_all(set)
        .await?
        .iter()
        .await?
        .next()
        .await
        .transpose()
}

pub(crate) async fn gca_all(this: &(impl DagAlgorithm + ?Sized), set: Set) -> Result<Set> {
    this.heads_ancestors(this.common_ancestors(set).await?)
        .await
}

pub(crate) async fn common_ancestors(this: &(impl DagAlgorithm + ?Sized), set: Set) -> Result<Set> {
    let result = match set.count_slow().await? {
        0 => set,
        1 => this.ancestors(set).await?,
        _ => {
            // Try to reduce the size of `set`.
            // `common_ancestors(X)` = `common_ancestors(roots(X))`.
            let set = this.roots(set).await?;
            let mut iter = set.iter().await?;
            let mut result = this
                .ancestors(Set::from(iter.next().await.unwrap()?))
                .await?;
            while let Some(v) = iter.next().await {
                result = result.intersection(&this.ancestors(Set::from(v?)).await?);
            }
            result
        }
    };
    Ok(result)
}

pub(crate) async fn is_ancestor(
    this: &(impl DagAlgorithm + ?Sized),
    ancestor: Vertex,
    descendant: Vertex,
) -> Result<bool> {
    let mut to_visit = vec![descendant];
    let mut visited: HashSet<_> = to_visit.clone().into_iter().collect();
    while let Some(v) = to_visit.pop() {
        if v == ancestor {
            return Ok(true);
        }
        for parent in this.parent_names(v).await? {
            if visited.insert(parent.clone()) {
                to_visit.push(parent);
            }
        }
    }
    Ok(false)
}

/// Implementation of `suggest_bisect`.
///
/// This is not the default trait implementation because the extra trait bounds
/// (ToIdSet, ToSet).
pub async fn suggest_bisect(
    this: &(impl DagAlgorithm + ToIdSet + ToSet + IdConvert + ?Sized),
    roots: Set,
    heads: Set,
    skip: Set,
) -> Result<(Option<Vertex>, Set, Set)> {
    let roots = this.to_id_set(&roots).await?;
    let heads = this.to_id_set(&heads).await?;
    let skip = this.to_id_set(&skip).await?;
    let (maybe_id, untested, heads) = this
        .id_dag_snapshot()?
        .suggest_bisect(&roots, &heads, &skip)?;
    let maybe_vertex = match maybe_id {
        Some(id) => Some(this.vertex_name(id).await?),
        None => None,
    };
    let untested = this.to_set(&untested)?;
    let heads = this.to_set(&heads)?;
    Ok((maybe_vertex, untested, heads))
}

// `scope` is usually the "dirty" set that might need to be inserted, or might
// already exist in the existing dag, obtained by `dag.dirty()`. It is okay for
// `scope` to be empty, which might lead to more network round-trips. See also
// the docstring for `Parents::hint_subdag_for_insertion`.
#[tracing::instrument(skip(this), level=tracing::Level::DEBUG)]
pub(crate) async fn hint_subdag_for_insertion(
    this: &(impl Parents + ?Sized),
    scope: &Set,
    heads: &[Vertex],
) -> Result<MemDag> {
    let count = scope.count_slow().await?;
    tracing::trace!("hint_subdag_for_insertion: pending vertexes: {}", count);

    // ScopedParents only contains parents within "scope".
    struct ScopedParents<'a, P: Parents + ?Sized> {
        parents: &'a P,
        scope: &'a Set,
    }

    #[async_trait::async_trait]
    impl<'a, P: Parents + ?Sized> Parents for ScopedParents<'a, P> {
        async fn parent_names(&self, name: Vertex) -> Result<Vec<Vertex>> {
            let parents: Vec<Vertex> = self.parents.parent_names(name).await?;
            // Filter by scope. We don't need to provide a "correct" parents here.
            // It is only used to optimize network fetches, not used to actually insert
            // to the graph.
            let mut filtered_parents = Vec::with_capacity(parents.len());
            for v in parents {
                if self.scope.contains(&v).await? {
                    filtered_parents.push(v)
                }
            }
            Ok(filtered_parents)
        }

        async fn hint_subdag_for_insertion(&self, _heads: &[Vertex]) -> Result<MemDag> {
            // No need to use such a hint (to avoid infinite recursion).
            // Pending names should exist in the graph without using remote fetching.
            Ok(MemDag::new())
        }
    }

    // Insert vertexes in `scope` to `dag`.
    let mut dag = MemDag::new();
    // The MemDag should not be lazy.
    assert!(!dag.is_vertex_lazy());

    let scoped_parents = ScopedParents {
        parents: this,
        scope,
    };

    // Exclude heads that are outside 'scope'. They might trigger remote fetches.
    let heads_in_scope = {
        let mut heads_in_scope = Vec::with_capacity(heads.len());
        for head in heads {
            if scope.contains(head).await? {
                heads_in_scope.push(head.clone());
            }
        }
        heads_in_scope
    };
    dag.add_heads(&scoped_parents, &heads_in_scope.into())
        .await?;

    Ok(dag)
}
