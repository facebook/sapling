/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use nonblocking::non_blocking;

use crate::ops::DagAlgorithm;
use crate::NameSet;
use crate::Result;
use crate::VerLink;
use crate::VertexName;

/// The DummyDag implements a DAG that contains all vertexes with no parents.
#[derive(Debug, Clone)]
pub(crate) struct DummyDag {
    version: VerLink,
}

impl DummyDag {
    pub fn new() -> Self {
        Self {
            version: VerLink::new(),
        }
    }
}

#[async_trait::async_trait]
impl DagAlgorithm for DummyDag {
    async fn sort(&self, set: &NameSet) -> Result<NameSet> {
        Ok(set.clone())
    }

    /// Get ordered parent vertexes.
    async fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        let _ = name;
        Ok(Vec::new())
    }

    /// Returns a set that covers all vertexes tracked by this DAG.
    async fn all(&self) -> Result<NameSet> {
        crate::errors::programming("DummyDag does not support all()")
    }

    /// Returns a set that covers all vertexes in the master group.
    async fn master_group(&self) -> Result<NameSet> {
        crate::errors::programming("DummyDag does not support master_group()")
    }

    /// Vertexes buffered in memory, not yet written to disk.
    async fn dirty(&self) -> Result<NameSet> {
        Ok(NameSet::empty())
    }

    /// Calculates all ancestors reachable from any name from the given set.
    async fn ancestors(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    /// Calculates parents of the given set.
    async fn parents(&self, set: NameSet) -> Result<NameSet> {
        let _ = set;
        Ok(NameSet::empty())
    }

    /// Calculates the n-th first ancestor.
    async fn first_ancestor_nth(&self, name: VertexName, n: u64) -> Result<Option<VertexName>> {
        if n == 0 {
            Ok(Some(name))
        } else {
            crate::errors::programming("DummyDag does not resolve x~n where n > 1")
        }
    }

    /// Calculates heads of the given set.
    async fn heads(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    /// Calculates children of the given set.
    async fn children(&self, set: NameSet) -> Result<NameSet> {
        let _ = set;
        Ok(NameSet::empty())
    }

    /// Calculates roots of the given set.
    async fn roots(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    /// Calculates one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    async fn gca_one(&self, set: NameSet) -> Result<Option<VertexName>> {
        if non_blocking(set.count())?? == 1 {
            non_blocking(set.first())?
        } else {
            Ok(None)
        }
    }

    /// Calculates all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    async fn gca_all(&self, set: NameSet) -> Result<NameSet> {
        self.common_ancestors(set).await
    }

    /// Calculates all common ancestors of the given set.
    async fn common_ancestors(&self, set: NameSet) -> Result<NameSet> {
        if non_blocking(set.count())?? == 1 {
            Ok(set)
        } else {
            Ok(NameSet::empty())
        }
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    async fn is_ancestor(&self, ancestor: VertexName, descendant: VertexName) -> Result<bool> {
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
    async fn heads_ancestors(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    /// Calculates the "dag range" - vertexes reachable from both sides.
    async fn range(&self, roots: NameSet, heads: NameSet) -> Result<NameSet> {
        Ok(roots & heads)
    }

    /// Calculates the descendants of the given set.
    async fn descendants(&self, set: NameSet) -> Result<NameSet> {
        Ok(set)
    }

    fn is_vertex_lazy(&self) -> bool {
        false
    }

    /// Get a snapshot of the current graph.
    fn dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        Ok(Arc::new(self.clone()))
    }

    fn dag_id(&self) -> &str {
        "dummy_dag"
    }

    fn dag_version(&self) -> &VerLink {
        &self.version
    }
}
