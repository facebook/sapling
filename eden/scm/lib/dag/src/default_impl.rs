/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::programming;
use crate::namedag::MemNameDag;
use crate::ops::DagAddHeads;
use crate::DagAlgorithm;
use crate::NameSet;
use crate::Result;
use crate::VertexName;
use std::collections::HashMap;
use std::collections::HashSet;

/// Re-create the graph so it looks better when rendered.
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
/// be emitted first during iteration, if `ancestors(x) & main_branch`
/// contains larger vertexes. For example, if `main_branch` is `[C, D, E]`,
/// then `C` will be emitted first, and the returned DAG will have `all()`
/// output `[C, D, A, B, E]`. Practically, `main_branch` usually contains
/// "public" commits.
///
/// This function is expensive. Only run on small graphs.
///
/// This function is currently more optimized for "forking" cases. It is
/// not yet optimized for graphs with many merges.
pub(crate) fn beautify(
    this: &(impl DagAlgorithm + ?Sized),
    main_branch: Option<NameSet>,
) -> Result<MemNameDag> {
    // Find the "largest" branch.
    fn find_main_branch(
        get_ancestors: &impl Fn(&VertexName) -> Result<NameSet>,
        heads: &[VertexName],
    ) -> Result<NameSet> {
        let mut best_branch = NameSet::empty();
        let mut best_count = best_branch.count()?;
        for head in heads {
            let branch = get_ancestors(head)?;
            let count = branch.count()?;
            if count > best_count {
                best_count = count;
                best_branch = branch;
            }
        }
        Ok(best_branch)
    };

    // Sort heads recursively.
    fn sort(
        get_ancestors: &impl Fn(&VertexName) -> Result<NameSet>,
        heads: &mut [VertexName],
        main_branch: NameSet,
    ) -> Result<()> {
        if heads.len() <= 1 {
            return Ok(());
        }

        // Sort heads by "branching point" on the main branch.
        let mut branching_points: HashMap<VertexName, usize> = HashMap::with_capacity(heads.len());
        for head in heads.iter() {
            let count = (get_ancestors(head)? & main_branch.clone()).count()?;
            branching_points.insert(head.clone(), count);
        }
        heads.sort_by_key(|v| branching_points.get(v));

        // For heads with a same branching point, sort them recursively
        // using a different "main branch".
        let mut start = 0;
        let mut start_branching_point: Option<usize> = None;
        for end in 0..=heads.len() {
            let branching_point = heads
                .get(end)
                .and_then(|h| branching_points.get(&h).cloned());
            if branching_point != start_branching_point {
                if start + 1 < end {
                    let heads = &mut heads[start..end];
                    let main_branch = find_main_branch(get_ancestors, heads)?;
                    sort(get_ancestors, heads, main_branch)?;
                }
                start = end;
                start_branching_point = branching_point;
            }
        }

        Ok(())
    };

    let main_branch = main_branch.unwrap_or_else(NameSet::empty);
    let mut heads: Vec<_> = this
        .heads_ancestors(this.all()?)?
        .iter()?
        .collect::<Result<_>>()?;
    let get_ancestors = |head: &VertexName| this.ancestors(head.into());
    // Stabilize output if the sort key conflicts.
    heads.sort();
    sort(&get_ancestors, &mut heads[..], main_branch)?;

    let mut dag = MemNameDag::new();
    let get_parents = |v| this.parent_names(v);
    dag.add_heads(get_parents, &heads)?;
    Ok(dag)
}

pub(crate) fn parents(this: &(impl DagAlgorithm + ?Sized), set: NameSet) -> Result<NameSet> {
    let mut result: Vec<VertexName> = Vec::new();
    for vertex in set.iter()? {
        let parents = this.parent_names(vertex?)?;
        result.extend(parents);
    }
    Ok(NameSet::from_static_names(result))
}

pub(crate) fn first_ancestor_nth(
    this: &(impl DagAlgorithm + ?Sized),
    name: VertexName,
    n: u64,
) -> Result<VertexName> {
    let mut vertex = name.clone();
    for _ in 0..n {
        let parents = this.parent_names(vertex)?;
        if parents.is_empty() {
            return programming(format!("{:?}~{} cannot be resolved", name, n));
        }
        vertex = parents[0].clone();
    }
    Ok(vertex)
}

pub(crate) fn heads(this: &(impl DagAlgorithm + ?Sized), set: NameSet) -> Result<NameSet> {
    Ok(set.clone() - this.parents(set)?)
}

pub(crate) fn roots(this: &(impl DagAlgorithm + ?Sized), set: NameSet) -> Result<NameSet> {
    Ok(set.clone() - this.children(set)?)
}

pub(crate) fn reachable_roots(
    this: &(impl DagAlgorithm + ?Sized),
    roots: NameSet,
    heads: NameSet,
) -> Result<NameSet> {
    let heads_ancestors = this.ancestors(heads.clone())?;
    let roots = roots & heads_ancestors.clone(); // Filter out "bogus" roots.
    let only = heads_ancestors - this.ancestors(roots.clone())?;
    Ok(roots.clone() & (heads.clone() | this.parents(only)?))
}

pub(crate) fn heads_ancestors(
    this: &(impl DagAlgorithm + ?Sized),
    set: NameSet,
) -> Result<NameSet> {
    this.heads(this.ancestors(set)?)
}

pub(crate) fn only(
    this: &(impl DagAlgorithm + ?Sized),
    reachable: NameSet,
    unreachable: NameSet,
) -> Result<NameSet> {
    let reachable = this.ancestors(reachable)?;
    let unreachable = this.ancestors(unreachable)?;
    Ok(reachable - unreachable)
}

pub(crate) fn only_both(
    this: &(impl DagAlgorithm + ?Sized),
    reachable: NameSet,
    unreachable: NameSet,
) -> Result<(NameSet, NameSet)> {
    let reachable = this.ancestors(reachable)?;
    let unreachable = this.ancestors(unreachable)?;
    Ok((reachable - unreachable.clone(), unreachable))
}

pub(crate) fn gca_one(
    this: &(impl DagAlgorithm + ?Sized),
    set: NameSet,
) -> Result<Option<VertexName>> {
    this.gca_all(set)?.iter()?.next().transpose()
}

pub(crate) fn gca_all(this: &(impl DagAlgorithm + ?Sized), set: NameSet) -> Result<NameSet> {
    this.heads_ancestors(this.common_ancestors(set)?)
}

pub(crate) fn common_ancestors(
    this: &(impl DagAlgorithm + ?Sized),
    set: NameSet,
) -> Result<NameSet> {
    let result = match set.count()? {
        0 => set,
        1 => this.ancestors(set)?,
        _ => {
            // Try to reduce the size of `set`.
            // `common_ancestors(X)` = `common_ancestors(roots(X))`.
            let set = this.roots(set)?;
            let mut iter = set.iter()?;
            let mut result = this.ancestors(NameSet::from(iter.next().unwrap()?))?;
            for v in iter {
                result = result.intersection(&this.ancestors(NameSet::from(v?))?);
            }
            result
        }
    };
    Ok(result)
}

pub(crate) fn is_ancestor(
    this: &(impl DagAlgorithm + ?Sized),
    ancestor: VertexName,
    descendant: VertexName,
) -> Result<bool> {
    let mut to_visit = vec![descendant];
    let mut visited: HashSet<_> = to_visit.clone().into_iter().collect();
    while let Some(v) = to_visit.pop() {
        if v == ancestor {
            return Ok(true);
        }
        for parent in this.parent_names(v)? {
            if visited.insert(parent.clone()) {
                to_visit.push(parent);
            }
        }
    }
    Ok(false)
}
