/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::DagAlgorithm;
use crate::NameSet;
use crate::VertexName;
use anyhow::bail;
use anyhow::Result;
use std::collections::HashSet;

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
            bail!("{:?}~{} cannot be resolved", name, n);
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

pub(crate) fn heads_ancestors(
    this: &(impl DagAlgorithm + ?Sized),
    set: NameSet,
) -> Result<NameSet> {
    this.heads(this.ancestors(set)?)
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
