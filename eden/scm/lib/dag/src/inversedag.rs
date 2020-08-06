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

/// Inversed DAG. Parents become children.
#[derive(Clone)]
pub struct InverseDag {
    original_dag: Arc<dyn DagAlgorithm + Send + Sync>,
}

impl InverseDag {
    pub fn new(dag: Arc<dyn DagAlgorithm + Send + Sync>) -> Self {
        Self { original_dag: dag }
    }
}

impl DagAlgorithm for InverseDag {
    fn sort(&self, set: &NameSet) -> Result<NameSet> {
        self.original_dag.sort(set)
    }

    fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        let mut result = Vec::new();
        for v in self.original_dag.children(name.into())?.iter()? {
            result.push(v?);
        }
        Ok(result)
    }

    fn all(&self) -> Result<NameSet> {
        self.original_dag.all()
    }

    fn ancestors(&self, set: NameSet) -> Result<NameSet> {
        self.original_dag.descendants(set)
    }

    fn children(&self, set: NameSet) -> Result<NameSet> {
        self.original_dag.parents(set)
    }

    fn roots(&self, set: NameSet) -> Result<NameSet> {
        self.original_dag.heads(set)
    }

    fn range(&self, roots: NameSet, heads: NameSet) -> Result<NameSet> {
        self.original_dag.range(heads, roots)
    }

    fn descendants(&self, set: NameSet) -> Result<NameSet> {
        self.original_dag.ancestors(set)
    }
}
