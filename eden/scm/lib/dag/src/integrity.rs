/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Integrity checks.

use crate::iddag::IdDag;
use crate::iddagstore::IdDagStore;
use crate::idmap::IdMapAssignHead;
use crate::namedag::AbstractNameDag;
use crate::nameset::NameSet;
use crate::ops::CheckIntegrity;
use crate::ops::DagAlgorithm;
use crate::ops::Persist;
use crate::ops::TryClone;
use crate::Id;
use crate::Result;

#[async_trait::async_trait]
impl<IS, M, P, S> CheckIntegrity for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Persist + Send + Sync,
    P: TryClone + Send + Sync,
    S: TryClone + Persist + Send + Sync,
{
    async fn check_universal_ids(&self) -> Result<Vec<Id>> {
        let universal_ids: Vec<Id> = self.dag.universal_ids()?.into_iter().collect();
        tracing::debug!("{} universally known vertexes", universal_ids.len());
        let exists = self.map.contains_vertex_id_locally(&universal_ids).await?;
        let missing_ids = universal_ids
            .into_iter()
            .zip(exists)
            .filter_map(|(id, b)| if b { None } else { Some(id) })
            .collect();
        Ok(missing_ids)
    }

    async fn check_segments(&self) -> Result<Vec<String>> {
        unimplemented!()
    }

    async fn check_isomorphic_graph(
        &self,
        other: &dyn DagAlgorithm,
        heads: NameSet,
    ) -> Result<Vec<String>> {
        let _ = (other, heads);
        unimplemented!();
    }
}
