/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use parking_lot::RwLock;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::idmap::IdMap;
use crate::idmap::IdMapVersion;
use crate::DagId;

#[derive(Debug)]
pub struct MemIdMap {
    dag_id2cs: HashMap<DagId, ChangesetId>,
    cs2dag_id: HashMap<ChangesetId, DagId>,
    last_entry: Option<(DagId, ChangesetId)>,
}

impl MemIdMap {
    pub fn new() -> Self {
        Self {
            dag_id2cs: HashMap::new(),
            cs2dag_id: HashMap::new(),
            last_entry: None,
        }
    }

    pub fn len(&self) -> usize {
        self.dag_id2cs.len()
    }

    pub fn drain(&mut self) -> impl Iterator<Item = (DagId, ChangesetId)> + '_ {
        self.last_entry = None;
        self.cs2dag_id.clear();
        self.dag_id2cs.drain()
    }

    pub fn insert(&mut self, dag_id: DagId, cs_id: ChangesetId) {
        self.dag_id2cs.insert(dag_id, cs_id);
        self.cs2dag_id.insert(cs_id, dag_id);
        match self.last_entry {
            Some((last_dag_id, _)) if last_dag_id > dag_id => {}
            _ => self.last_entry = Some((dag_id, cs_id)),
        }
    }

    pub fn find_changeset_id(&self, dag_id: DagId) -> Option<ChangesetId> {
        self.dag_id2cs.get(&dag_id).copied()
    }

    pub fn find_dag_id(&self, cs_id: ChangesetId) -> Option<DagId> {
        self.cs2dag_id.get(&cs_id).copied()
    }

    pub fn get_last_entry(&self) -> Result<Option<(DagId, ChangesetId)>> {
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

    pub fn len(&self) -> usize {
        let inner = self.inner.read();
        inner.len()
    }

    pub fn drain(&self) -> Vec<(DagId, ChangesetId)> {
        let mut inner = self.inner.write();
        inner.drain().collect()
    }
}

#[async_trait::async_trait]
impl IdMap for ConcurrentMemIdMap {
    async fn insert_many(
        &self,
        _ctx: &CoreContext,
        mappings: Vec<(DagId, ChangesetId)>,
    ) -> Result<()> {
        let mut inner = self.inner.write();
        for (dag_id, cs) in mappings {
            inner.insert(dag_id, cs);
        }
        Ok(())
    }

    async fn find_many_changeset_ids(
        &self,
        _ctx: &CoreContext,
        dag_ids: Vec<DagId>,
    ) -> Result<HashMap<DagId, ChangesetId>> {
        let inner = self.inner.read();
        let result = dag_ids
            .into_iter()
            .filter_map(|v| inner.find_changeset_id(v).map(|cs| (v, cs)))
            .collect();
        Ok(result)
    }

    async fn find_many_dag_ids(
        &self,
        _ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        let inner = self.inner.read();
        let result = cs_ids
            .into_iter()
            .filter_map(|cs| inner.find_dag_id(cs).map(|v| (cs, v)))
            .collect();
        Ok(result)
    }

    async fn find_many_dag_ids_maybe_stale(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        self.find_many_dag_ids(ctx, cs_ids).await
    }

    async fn get_last_entry(&self, _ctx: &CoreContext) -> Result<Option<(DagId, ChangesetId)>> {
        let inner = self.inner.read();
        inner.get_last_entry()
    }

    fn idmap_version(&self) -> Option<IdMapVersion> {
        None
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use maplit::hashmap;

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::AS_CSID;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;

    #[fbinit::test]
    async fn test_concurrent_mem_idmap(fb: FacebookInit) -> Result<()> {
        let idmap = ConcurrentMemIdMap::new();
        let ctx = CoreContext::test_mock(fb);

        assert_eq!(
            idmap
                .find_many_dag_ids(&ctx, vec![AS_CSID, ONES_CSID, TWOS_CSID])
                .await?,
            hashmap![]
        );
        assert_eq!(
            idmap
                .find_many_changeset_ids(&ctx, vec![DagId(0), DagId(1), DagId(2)])
                .await?,
            hashmap![]
        );
        assert_eq!(idmap.get_last_entry(&ctx).await?, None);

        idmap
            .insert_many(&ctx, vec![(DagId(0), AS_CSID), (DagId(1), ONES_CSID)])
            .await?;

        assert_eq!(
            idmap
                .find_many_dag_ids(&ctx, vec![AS_CSID, ONES_CSID, TWOS_CSID])
                .await?,
            hashmap![AS_CSID => DagId(0), ONES_CSID => DagId(1)]
        );
        assert_eq!(
            idmap
                .find_many_changeset_ids(&ctx, vec![DagId(0), DagId(1), DagId(2)])
                .await?,
            hashmap![DagId(0) => AS_CSID, DagId(1) => ONES_CSID]
        );
        assert_eq!(
            idmap.get_last_entry(&ctx).await?,
            Some((DagId(1), ONES_CSID))
        );

        Ok(())
    }
}
