/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod cache;
mod mem;
mod sql;
mod version;

pub use self::cache::{CacheHandlers, CachedIdMap};
pub use self::mem::{ConcurrentMemIdMap, MemIdMap};
pub use self::sql::SqlIdMap;
pub use self::version::SqlIdMapVersionStore;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, format_err, Context, Result};
use async_trait::async_trait;

use sql_ext::replication::ReplicaLagMonitor;
use sql_ext::SqlConnections;

use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};

use crate::types::IdMapVersion;
use crate::{DagId, Group, InProcessIdDag};

#[async_trait]
pub trait IdMap: Send + Sync {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mappings: Vec<(DagId, ChangesetId)>,
    ) -> Result<()>;

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        dag_ids: Vec<DagId>,
    ) -> Result<HashMap<DagId, ChangesetId>>;


    async fn find_many_dag_ids(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>>;

    /// Finds the dag ID for given changeset - if possible to do so quickly.
    /// Might return no answers for changesets that have dag ids assigned.
    ///
    /// Should be used by callers that can deal with missing information.
    async fn find_many_dag_ids_maybe_stale(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>>;

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(DagId, ChangesetId)>>;

    // Default implementations

    async fn insert(&self, ctx: &CoreContext, dag_id: DagId, cs_id: ChangesetId) -> Result<()> {
        self.insert_many(ctx, vec![(dag_id, cs_id)]).await
    }

    async fn find_changeset_id(
        &self,
        ctx: &CoreContext,
        dag_id: DagId,
    ) -> Result<Option<ChangesetId>> {
        Ok(self
            .find_many_changeset_ids(ctx, vec![dag_id])
            .await?
            .remove(&dag_id))
    }

    async fn find_dag_id(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Option<DagId>> {
        Ok(self
            .find_many_dag_ids(ctx, vec![cs_id])
            .await?
            .remove(&cs_id))
    }

    async fn find_dag_id_maybe_stale(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<DagId>> {
        Ok(self
            .find_many_dag_ids_maybe_stale(ctx, vec![cs_id])
            .await?
            .remove(&cs_id))
    }

    async fn get_changeset_id(&self, ctx: &CoreContext, dag_id: DagId) -> Result<ChangesetId> {
        self.find_changeset_id(ctx, dag_id)
            .await?
            .ok_or_else(|| format_err!("Failed to find segmented changelog id {} in IdMap", dag_id))
    }

    async fn get_dag_id(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<DagId> {
        self.find_dag_id(ctx, cs_id)
            .await?
            .ok_or_else(|| format_err!("Failed to find changeset id {} in IdMap", cs_id))
    }
}

#[async_trait]
impl IdMap for Arc<dyn IdMap> {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mappings: Vec<(DagId, ChangesetId)>,
    ) -> Result<()> {
        (**self).insert_many(ctx, mappings).await
    }

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        dag_ids: Vec<DagId>,
    ) -> Result<HashMap<DagId, ChangesetId>> {
        (**self).find_many_changeset_ids(ctx, dag_ids).await
    }

    async fn find_many_dag_ids(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        (**self).find_many_dag_ids(ctx, cs_ids).await
    }

    async fn find_many_dag_ids_maybe_stale(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        (**self).find_many_dag_ids_maybe_stale(ctx, cs_ids).await
    }

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(DagId, ChangesetId)>> {
        (**self).get_last_entry(ctx).await
    }
}

/// The idmap works in unison with the iddag. The idmap and the iddag need to be in sync for iddag
/// update operation to be consistent.  When we download an iddag, it has the idmap described by
/// the shared store. The overlay allows us to update the iddag in process by adding an idmap that
/// stores all the changes that we made in process. It's important to note that the shared store is
/// updated constantly by other processes. Because dag_ids are added in increasing order, we can
/// use the last entry in the downloaded iddag as the cutoff that delimitates the entries that are
/// fetched from the shared store and those that are fetched from the in process store. Note that
/// if we were to use the abcence of an entry from the in process store to fetch from the shared
/// store we would likely end up with inconsistent entries from an updated shared store.
// DagIds greater than or equal to cutoff are associated with mem idmap.
// DagIds less than cutoff are associated with shared idmap.
pub struct OverlayIdMap {
    mem: ConcurrentMemIdMap,
    shared: Arc<dyn IdMap>,
    cutoff: DagId,
}

impl OverlayIdMap {
    pub fn new(shared: Arc<dyn IdMap>, cutoff: DagId) -> Self {
        let mem = ConcurrentMemIdMap::new();
        Self {
            mem,
            shared,
            cutoff,
        }
    }

    pub fn from_iddag_and_idmap(iddag: &InProcessIdDag, shared: Arc<dyn IdMap>) -> Result<Self> {
        let cutoff = iddag
            .next_free_id(0, Group::MASTER)
            .context("error fetching next iddag id")?;
        Ok(Self::new(shared, cutoff))
    }
}

#[async_trait]
impl IdMap for OverlayIdMap {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mappings: Vec<(DagId, ChangesetId)>,
    ) -> Result<()> {
        for (v, _) in mappings.iter() {
            if v < &self.cutoff {
                return Err(format_err!(
                    "overlay idmap asked to insert {} but cutoff is {}",
                    v,
                    self.cutoff
                ));
            }
        }
        self.mem.insert_many(ctx, mappings).await
    }

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        dag_ids: Vec<DagId>,
    ) -> Result<HashMap<DagId, ChangesetId>> {
        let from_mem = dag_ids
            .iter()
            .filter(|&v| v >= &self.cutoff)
            .cloned()
            .collect();
        let mut result = self.mem.find_many_changeset_ids(ctx, from_mem).await?;
        let from_shared: Vec<DagId> = dag_ids
            .iter()
            .filter(|&v| v < &self.cutoff)
            .cloned()
            .collect();
        if !from_shared.is_empty() {
            let shared_result = self
                .shared
                .find_many_changeset_ids(ctx, from_shared)
                .await?;
            for (v, cs) in shared_result {
                result.insert(v, cs);
            }
        }
        Ok(result)
    }

    async fn find_many_dag_ids(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        let mut result = self.mem.find_many_dag_ids(ctx, cs_ids.clone()).await?;
        for (cs, v) in result.iter() {
            if v < &self.cutoff {
                bail!(
                    "unexpected assignment found in mem idmap: {} for {} but cutoff is {}",
                    v,
                    cs,
                    self.cutoff
                );
            }
        }
        let to_get_shared = cs_ids
            .into_iter()
            .filter(|cs_id| !result.contains_key(&cs_id))
            .collect();
        let from_shared = self.shared.find_many_dag_ids(ctx, to_get_shared).await?;
        for (cs, v) in from_shared {
            if v < self.cutoff {
                result.insert(cs, v);
            }
        }
        Ok(result)
    }

    async fn find_many_dag_ids_maybe_stale(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        let mut result = self
            .mem
            .find_many_dag_ids_maybe_stale(ctx, cs_ids.clone())
            .await?;
        for (cs, v) in result.iter() {
            if v < &self.cutoff {
                bail!(
                    "unexpected assignment found in mem idmap: {} for {} but cutoff is {}",
                    v,
                    cs,
                    self.cutoff
                );
            }
        }
        let to_get_shared = cs_ids
            .into_iter()
            .filter(|cs_id| !result.contains_key(&cs_id))
            .collect();
        let from_shared = self
            .shared
            .find_many_dag_ids_maybe_stale(ctx, to_get_shared)
            .await?;
        for (cs, v) in from_shared {
            if v < self.cutoff {
                result.insert(cs, v);
            }
        }
        Ok(result)
    }

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(DagId, ChangesetId)>> {
        match self.mem.get_last_entry(ctx).await? {
            Some(x) => Ok(Some(x)),
            None if self.cutoff > DagId(0) => {
                let dag_id = self.cutoff - 1;
                let cs_id = self
                    .shared
                    .find_changeset_id(ctx, dag_id)
                    .await?
                    .with_context(|| {
                        format!(
                            "could not find shared entry for dag_id {} (overlay cutoff = {})",
                            dag_id, self.cutoff
                        )
                    })?;
                Ok(Some((dag_id, cs_id)))
            }
            None => Ok(None),
        }
    }
}

// The builder for the standard IdMap
// Our layers are: SqlIdMap, CachedIdMap, OverlayIdMap
#[derive(Clone)]
pub struct IdMapFactory {
    connections: SqlConnections,
    replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
    repo_id: RepositoryId,
    cache_handlers: Option<CacheHandlers>,
}

impl IdMapFactory {
    pub fn new(
        connections: SqlConnections,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
        repo_id: RepositoryId,
    ) -> Self {
        Self {
            connections,
            replica_lag_monitor,
            repo_id,
            cache_handlers: None,
        }
    }

    // Writes go to the SQL table.
    pub fn for_writer(&self, ctx: &CoreContext, version: IdMapVersion) -> Arc<dyn IdMap> {
        let sql_idmap = SqlIdMap::new(
            self.connections.clone(),
            self.replica_lag_monitor.clone(),
            self.repo_id,
            version,
        );
        slog::debug!(
            ctx.logger(),
            "segmented changelog idmap instantiated - version: {}",
            version
        );
        let mut idmap: Arc<dyn IdMap> = Arc::new(sql_idmap);
        if let Some(cache_handlers) = &self.cache_handlers {
            idmap = Arc::new(CachedIdMap::new(
                idmap,
                cache_handlers.clone(),
                self.repo_id,
                version,
            ));
        }
        idmap
    }

    // Servers have an overlay idmap which means that all their updates to the idmap stay confined
    // to the Dag that performed the updates.
    pub fn for_server(
        &self,
        ctx: &CoreContext,
        version: IdMapVersion,
        iddag: &InProcessIdDag,
    ) -> Result<Arc<dyn IdMap>> {
        let overlay = OverlayIdMap::from_iddag_and_idmap(iddag, self.for_writer(ctx, version))?;
        Ok(Arc::new(overlay))
    }

    pub fn with_cache_handlers(mut self, cache_handlers: CacheHandlers) -> Self {
        self.cache_handlers = Some(cache_handlers);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use maplit::hashmap;

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::{
        AS_CSID, FOURS_CSID, ONES_CSID, THREES_CSID, TWOS_CSID,
    };

    #[fbinit::test]
    async fn test_overlay_idmap_find(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let shared: Arc<dyn IdMap> = Arc::new(ConcurrentMemIdMap::new());

        shared
            .insert_many(&ctx, vec![(DagId(0), AS_CSID), (DagId(1), ONES_CSID)])
            .await?;

        let overlay = OverlayIdMap::new(Arc::clone(&shared), DagId(2));

        assert_eq!(
            overlay
                .find_many_changeset_ids(&ctx, vec![DagId(0), DagId(1), DagId(2)])
                .await?,
            hashmap![DagId(0) => AS_CSID, DagId(1) => ONES_CSID]
        );

        overlay
            .insert_many(&ctx, vec![(DagId(2), TWOS_CSID), (DagId(3), THREES_CSID)])
            .await?;
        assert_eq!(
            overlay
                .find_many_changeset_ids(&ctx, vec![DagId(2), DagId(3)])
                .await?,
            hashmap![DagId(2) => TWOS_CSID, DagId(3) => THREES_CSID]
        );

        assert_eq!(
            overlay
                .shared
                .find_many_changeset_ids(&ctx, vec![DagId(2), DagId(3)])
                .await?,
            hashmap![]
        );

        shared
            .insert_many(
                &ctx,
                vec![
                    (DagId(2), THREES_CSID),
                    (DagId(3), TWOS_CSID),
                    (DagId(4), FOURS_CSID),
                ],
            )
            .await?;

        assert_eq!(
            overlay
                .find_many_changeset_ids(&ctx, vec![DagId(3), DagId(4)])
                .await?,
            hashmap![DagId(3) => THREES_CSID]
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_overlay_idmap_last_entry(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let shared: Arc<dyn IdMap> = Arc::new(ConcurrentMemIdMap::new());

        shared
            .insert_many(&ctx, vec![(DagId(0), AS_CSID), (DagId(1), ONES_CSID)])
            .await?;

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), DagId(0));
            assert_eq!(overlay.get_last_entry(&ctx).await?, None);
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), DagId(1));
            assert_eq!(
                overlay.get_last_entry(&ctx).await?,
                Some((DagId(0), AS_CSID))
            );
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), DagId(1));
            overlay.insert(&ctx, DagId(1), THREES_CSID).await?;
            assert_eq!(
                overlay.get_last_entry(&ctx).await?,
                Some((DagId(1), THREES_CSID)),
            );
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), DagId(2));
            assert_eq!(
                overlay.get_last_entry(&ctx).await?,
                Some((DagId(1), ONES_CSID)),
            );
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), DagId(2));
            overlay.insert(&ctx, DagId(2), TWOS_CSID).await?;
            assert_eq!(
                overlay.get_last_entry(&ctx).await?,
                Some((DagId(2), TWOS_CSID)),
            );
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), DagId(3));
            assert!(overlay.get_last_entry(&ctx).await.is_err());
        }


        Ok(())
    }
}
