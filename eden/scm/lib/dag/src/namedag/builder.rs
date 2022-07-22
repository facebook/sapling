/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use crate::namedag::AbstractNameDag;
use crate::IdDag;
use crate::IdDagStore;

/// State to build a new `AbstractNameDag`.
pub struct NameDagBuilder<M, D, P, S> {
    map: M,
    dag: D,
    path: P,
    state: S,
    id: String,
}

impl<M, D> NameDagBuilder<M, D, (), ()> {
    /// Create the builder with specified `IdMap` and `IdDag`.
    ///
    /// The callsite must ensure the `IdMap` and `IdMap` are
    /// in sync.
    pub fn new_with_idmap_dag(map: M, dag: D) -> Self {
        Self {
            map,
            dag,
            path: (),
            id: String::new(),
            state: (),
        }
    }
}

impl<M, D, P, S> NameDagBuilder<M, D, P, S> {
    /// Set the `path`, used to re-open the `NameDag`.
    pub fn with_path<P2>(self, path: P2) -> NameDagBuilder<M, D, P2, S> {
        NameDagBuilder {
            map: self.map,
            dag: self.dag,
            path,
            id: self.id,
            state: self.state,
        }
    }

    /// Set the `state`, additional state maintained used by the callsite.
    pub fn with_state<S2>(self, state: S2) -> NameDagBuilder<M, D, P, S2> {
        NameDagBuilder {
            map: self.map,
            dag: self.dag,
            path: self.path,
            id: self.id,
            state,
        }
    }

    /// Set the `id`, used for debugging.
    pub fn with_id(self, id: String) -> NameDagBuilder<M, D, P, S> {
        NameDagBuilder {
            map: self.map,
            dag: self.dag,
            path: self.path,
            id,
            state: self.state,
        }
    }
}

impl<IS, M, P, S> NameDagBuilder<M, IdDag<IS>, P, S>
where
    M: Send + Sync,
    IS: IdDagStore,
    P: Send + Sync,
    S: Send + Sync,
{
    /// Build the `AbstractNameDag`.
    pub fn build(self) -> crate::Result<AbstractNameDag<IdDag<IS>, M, P, S>> {
        let persisted_id_set = self.dag.all()?;
        let overlay_map_id_set = self.dag.master_group()?;
        let dag = AbstractNameDag {
            dag: self.dag,
            map: self.map,
            path: self.path,
            state: self.state,
            id: self.id,

            snapshot: Default::default(),
            pending_heads: Default::default(),
            persisted_id_set,
            overlay_map: Default::default(),
            overlay_map_id_set,
            overlay_map_paths: Default::default(),
            remote_protocol: Arc::new(()),
            missing_vertexes_confirmed_by_remote: Default::default(),
        };
        Ok(dag)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::iddag::IdDag;
    use crate::idmap::MemIdMap;
    use crate::ops::DagAddHeads;
    use crate::VertexListWithOptions;

    #[tokio::test]
    async fn test_builder_absent_path_state_can_use_add_heads() {
        let dag = IdDag::new_in_process();
        let map = MemIdMap::new();
        let builder = NameDagBuilder::new_with_idmap_dag(map, dag);
        let mut dag = builder.build().unwrap();
        dag.add_heads(&HashMap::new(), &VertexListWithOptions::default())
            .await
            .unwrap();
    }
}
