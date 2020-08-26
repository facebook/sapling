/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ops::DagAlgorithm;
use crate::NameSet;
use crate::Result;
use crate::VertexName;
use std::sync::Arc;

/// The DummyDag implements a DAG that contains all vertexes with no parents.
#[derive(Debug, Copy, Clone)]
pub(crate) struct DummyDag;

impl DagAlgorithm for DummyDag {
    fn sort(&self, set: &NameSet) -> Result<NameSet> {
        Ok(set.clone())
    }

    /// Get ordered parent vertexes.
    fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        let _ = name;
        Ok(Vec::new())
    }

    /// Returns a [`SpanSet`] that covers all vertexes tracked by this DAG.
    fn all(&self) -> Result<NameSet> {
        crate::errors::programming("DummyDag does not support all()")
    }

    /// Calculates all ancestors reachable from any name from the given set.
    fn ancestors(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    /// Calculates parents of the given set.
    fn parents(&self, set: NameSet) -> Result<NameSet> {
        let _ = set;
        Ok(NameSet::empty())
    }

    /// Calculates the n-th first ancestor.
    fn first_ancestor_nth(&self, name: VertexName, n: u64) -> Result<VertexName> {
        if n == 0 {
            Ok(name)
        } else {
            crate::errors::programming("DummyDag does not resolve x~n where n > 1")
        }
    }

    /// Calculates heads of the given set.
    fn heads(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    /// Calculates children of the given set.
    fn children(&self, set: NameSet) -> Result<NameSet> {
        let _ = set;
        Ok(NameSet::empty())
    }

    /// Calculates roots of the given set.
    fn roots(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    /// Calculates one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    fn gca_one(&self, set: NameSet) -> Result<Option<VertexName>> {
        if set.count()? == 1 {
            set.first()
        } else {
            Ok(None)
        }
    }

    /// Calculates all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    fn gca_all(&self, set: NameSet) -> Result<NameSet> {
        self.common_ancestors(set)
    }

    /// Calculates all common ancestors of the given set.
    fn common_ancestors(&self, set: NameSet) -> Result<NameSet> {
        if set.count()? == 1 {
            Ok(set)
        } else {
            Ok(NameSet::empty())
        }
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    fn is_ancestor(&self, ancestor: VertexName, descendant: VertexName) -> Result<bool> {
        Ok(ancestor == descendant)
    }

    /// Calculates "heads" of the ancestors of the given set. That is,
    /// Find Y, which is the smallest subset of set X, where `ancestors(Y)` is
    /// `ancestors(X)`.
    ///
    /// This is faster than calculating `heads(ancestors(set))`.
    ///
    /// This is different from `heads`. In case set contains X and Y, and Y is
    /// an ancestor of X, but not the immediate ancestor, `heads` will include
    /// Y while this function won't.
    fn heads_ancestors(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    /// Calculates the "dag range" - vertexes reachable from both sides.
    fn range(&self, roots: NameSet, heads: NameSet) -> Result<NameSet> {
        Ok(roots & heads)
    }

    /// Calculates the descendants of the given set.
    fn descendants(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    /// Get a snapshot of the current graph.
    fn dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        Ok(Arc::new(DummyDag))
    }
}
