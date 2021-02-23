/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{format_err, Result};
use parking_lot::RwLock;

use dag::Id as Vertex;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::idmap::IdMap;

#[derive(Debug)]
pub struct MemIdMap {
    vertex2cs: HashMap<Vertex, ChangesetId>,
    cs2vertex: HashMap<ChangesetId, Vertex>,
    last_entry: Option<(Vertex, ChangesetId)>,
}

impl MemIdMap {
    pub fn new() -> Self {
        Self {
            vertex2cs: HashMap::new(),
            cs2vertex: HashMap::new(),
            last_entry: None,
        }
    }

    pub fn len(&self) -> usize {
        self.vertex2cs.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Vertex, ChangesetId)> + '_ {
        self.vertex2cs
            .iter()
            .map(|(&vertex, &cs_id)| (vertex, cs_id))
    }

    pub fn insert(&mut self, vertex: Vertex, cs_id: ChangesetId) {
        self.vertex2cs.insert(vertex, cs_id);
        self.cs2vertex.insert(cs_id, vertex);
        match self.last_entry {
            Some((last_vertex, _)) if last_vertex > vertex => {}
            _ => self.last_entry = Some((vertex, cs_id)),
        }
    }

    pub fn find_changeset_id(&self, vertex: Vertex) -> Option<ChangesetId> {
        self.vertex2cs.get(&vertex).copied()
    }

    pub fn get_changeset_id(&self, vertex: Vertex) -> Result<ChangesetId> {
        self.find_changeset_id(vertex)
            .ok_or_else(|| format_err!("Failed to find segmented changelog id {} in IdMap", vertex))
    }

    pub fn find_vertex(&self, cs_id: ChangesetId) -> Option<Vertex> {
        self.cs2vertex.get(&cs_id).copied()
    }

    pub fn get_vertex(&self, cs_id: ChangesetId) -> Result<Vertex> {
        self.find_vertex(cs_id)
            .ok_or_else(|| format_err!("Failed to find changeset id {} in IdMap", cs_id))
    }

    pub fn get_last_entry(&self) -> Result<Option<(Vertex, ChangesetId)>> {
        Ok(self.last_entry.clone())
    }
}

#[derive(Debug)]
pub struct ConcurrentMemIdMap {
    inner: RwLock<MemIdMap>,
}

impl ConcurrentMemIdMap {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(MemIdMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl IdMap for ConcurrentMemIdMap {
    async fn insert_many(
        &self,
        _ctx: &CoreContext,
        mappings: Vec<(Vertex, ChangesetId)>,
    ) -> Result<()> {
        let mut inner = self.inner.write();
        for (vertex, cs) in mappings {
            inner.insert(vertex, cs);
        }
        Ok(())
    }

    async fn find_many_changeset_ids(
        &self,
        _ctx: &CoreContext,
        vertexes: Vec<Vertex>,
    ) -> Result<HashMap<Vertex, ChangesetId>> {
        let inner = self.inner.read();
        let result = vertexes
            .into_iter()
            .filter_map(|v| inner.find_changeset_id(v).map(|cs| (v, cs)))
            .collect();
        Ok(result)
    }

    async fn find_many_vertexes(
        &self,
        _ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Vertex>> {
        let inner = self.inner.read();
        let result = cs_ids
            .into_iter()
            .filter_map(|cs| inner.find_vertex(cs).map(|v| (cs, v)))
            .collect();
        Ok(result)
    }

    async fn get_last_entry(&self, _ctx: &CoreContext) -> Result<Option<(Vertex, ChangesetId)>> {
        let inner = self.inner.read();
        inner.get_last_entry()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use maplit::hashmap;

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::{AS_CSID, ONES_CSID, TWOS_CSID};

    #[fbinit::compat_test]
    async fn test_concurrent_mem_idmap(fb: FacebookInit) -> Result<()> {
        let idmap = ConcurrentMemIdMap::new();
        let ctx = CoreContext::test_mock(fb);

        assert_eq!(
            idmap
                .find_many_vertexes(&ctx, vec![AS_CSID, ONES_CSID, TWOS_CSID])
                .await?,
            hashmap![]
        );
        assert_eq!(
            idmap
                .find_many_changeset_ids(&ctx, vec![Vertex(0), Vertex(1), Vertex(2)])
                .await?,
            hashmap![]
        );
        assert_eq!(idmap.get_last_entry(&ctx).await?, None);

        idmap
            .insert_many(&ctx, vec![(Vertex(0), AS_CSID), (Vertex(1), ONES_CSID)])
            .await?;

        assert_eq!(
            idmap
                .find_many_vertexes(&ctx, vec![AS_CSID, ONES_CSID, TWOS_CSID])
                .await?,
            hashmap![AS_CSID => Vertex(0), ONES_CSID => Vertex(1)]
        );
        assert_eq!(
            idmap
                .find_many_changeset_ids(&ctx, vec![Vertex(0), Vertex(1), Vertex(2)])
                .await?,
            hashmap![Vertex(0) => AS_CSID, Vertex(1) => ONES_CSID]
        );
        assert_eq!(
            idmap.get_last_entry(&ctx).await?,
            Some((Vertex(1), ONES_CSID))
        );

        Ok(())
    }
}
