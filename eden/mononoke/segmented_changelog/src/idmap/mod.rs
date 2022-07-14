/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod cache;
mod mem;
mod shared_traits;
mod sql;

pub use self::cache::CacheHandlers;
pub use self::cache::CachedIdMap;
pub use self::mem::ConcurrentMemIdMap;
pub use self::mem::MemIdMap;
pub use self::shared_traits::cs_id_from_vertex_name;
pub use self::shared_traits::vertex_name_from_cs_id;
pub use self::shared_traits::IdMapWrapper;
pub use self::sql::SqlIdMap;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;

use sql_ext::replication::ReplicaLagMonitor;
use sql_ext::SqlConnections;

use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;

use crate::types::IdMapVersion;
use crate::DagId;
use crate::DagIdSet;
use crate::InProcessIdDag;

#[async_trait]
#[auto_impl::auto_impl(&, Arc)]
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

    fn idmap_version(&self) -> Option<IdMapVersion>;

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

/// The idmap works in unison with the iddag. The idmap and the iddag need to be in sync for iddag
/// update operation to be consistent.  When we download an iddag, it has the idmap described by
/// the shared store. The overlay allows us to update the iddag in process by adding an idmap that
/// stores all the changes that we made in process. It's important to note that the shared store is
/// updated constantly by other processes. So we need to be careful not using the shared store
/// for Ids that are not referred by the matching IdDag. To do that, we ask the IdDag about all
/// Ids covered by it, store it in `shared_id_set` and then only use the shared store for Ids in
/// `shared_id_set`. Note that if we were to use the abcence of an entry from the in process store
/// to fetch from the shared store we would likely end up with inconsistent entries from an updated
/// shared store.
pub struct OverlayIdMap {
    /// Source for `Id`s not in `shared_id_set`. Mutable.
    mem: ConcurrentMemIdMap,
    /// Source for `Id`s in `shared_id_set`. Immutable.
    shared: Arc<dyn IdMap>,
    /// `Id`s covered by the `shared` IdMap.
    shared_id_set: DagIdSet,
}

impl OverlayIdMap {
    pub fn new(shared: Arc<dyn IdMap>, shared_id_set: DagIdSet) -> Self {
        let mem = ConcurrentMemIdMap::new();
        Self {
            mem,
            shared,
            shared_id_set,
        }
    }

    pub fn from_iddag_and_idmap(iddag: &InProcessIdDag, shared: Arc<dyn IdMap>) -> Result<Self> {
        let shared_id_set = iddag.all().context("error calculating iddag.all()")?;
        Ok(Self::new(shared, shared_id_set))
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
            if self.shared_id_set.contains(*v) {
                return Err(format_err!(
                    "overlay idmap asked to insert {} but it is in immutable shared set {:?}",
                    v,
                    &self.shared_id_set
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
            .filter(|&v| !self.shared_id_set.contains(*v))
            .cloned()
            .collect();
        let mut result = self.mem.find_many_changeset_ids(ctx, from_mem).await?;
        let from_shared: Vec<DagId> = dag_ids
            .iter()
            .filter(|&v| self.shared_id_set.contains(*v))
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
            if self.shared_id_set.contains(*v) {
                bail!(
                    "unexpected assignment found in mem idmap: {} for {} but shared_id_set is {:?}",
                    v,
                    cs,
                    &self.shared_id_set
                );
            }
        }
        let to_get_shared = cs_ids
            .into_iter()
            .filter(|cs_id| !result.contains_key(cs_id))
            .collect();
        let from_shared = self.shared.find_many_dag_ids(ctx, to_get_shared).await?;
        for (cs, v) in from_shared {
            if self.shared_id_set.contains(v) {
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
            if self.shared_id_set.contains(*v) {
                bail!(
                    "unexpected assignment found in mem idmap: {} for {} but shared_id_set is {:?}",
                    v,
                    cs,
                    &self.shared_id_set
                );
            }
        }
        let to_get_shared = cs_ids
            .into_iter()
            .filter(|cs_id| !result.contains_key(cs_id))
            .collect();
        let from_shared = self
            .shared
            .find_many_dag_ids_maybe_stale(ctx, to_get_shared)
            .await?;
        for (cs, v) in from_shared {
            if self.shared_id_set.contains(v) {
                result.insert(cs, v);
            }
        }
        Ok(result)
    }

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(DagId, ChangesetId)>> {
        match self.mem.get_last_entry(ctx).await? {
            Some(x) => Ok(Some(x)),
            None if !self.shared_id_set.is_empty() => {
                let dag_id = self.shared_id_set.max().unwrap();
                let cs_id = self
                    .shared
                    .find_changeset_id(ctx, dag_id)
                    .await?
                    .with_context(|| {
                        format!(
                            "could not find shared entry for dag_id {} (overlay shared_id_set = {:?})",
                            dag_id, &self.shared_id_set
                        )
                    })?;
                Ok(Some((dag_id, cs_id)))
            }
            None => Ok(None),
        }
    }

    fn idmap_version(&self) -> Option<IdMapVersion> {
        self.shared.idmap_version()
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

    use mononoke_types_mocks::changesetid::AS_CSID;
    use mononoke_types_mocks::changesetid::FOURS_CSID;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;

    fn cutoff(n: u64) -> DagIdSet {
        if n == 0 {
            DagIdSet::empty()
        } else {
            DagIdSet::from_spans(vec![DagId(0)..=DagId(n - 1)])
        }
    }

    #[fbinit::test]
    async fn test_overlay_idmap_find(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let shared: Arc<dyn IdMap> = Arc::new(ConcurrentMemIdMap::new());

        shared
            .insert_many(&ctx, vec![(DagId(0), AS_CSID), (DagId(1), ONES_CSID)])
            .await?;

        let overlay = OverlayIdMap::new(Arc::clone(&shared), cutoff(2));

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
            let overlay = OverlayIdMap::new(Arc::clone(&shared), cutoff(0));
            assert_eq!(overlay.get_last_entry(&ctx).await?, None);
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), cutoff(1));
            assert_eq!(
                overlay.get_last_entry(&ctx).await?,
                Some((DagId(0), AS_CSID))
            );
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), cutoff(1));
            overlay.insert(&ctx, DagId(1), THREES_CSID).await?;
            assert_eq!(
                overlay.get_last_entry(&ctx).await?,
                Some((DagId(1), THREES_CSID)),
            );
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), cutoff(2));
            assert_eq!(
                overlay.get_last_entry(&ctx).await?,
                Some((DagId(1), ONES_CSID)),
            );
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), cutoff(2));
            overlay.insert(&ctx, DagId(2), TWOS_CSID).await?;
            assert_eq!(
                overlay.get_last_entry(&ctx).await?,
                Some((DagId(2), TWOS_CSID)),
            );
        }

        {
            let overlay = OverlayIdMap::new(Arc::clone(&shared), cutoff(3));
            assert!(overlay.get_last_entry(&ctx).await.is_err());
        }

        Ok(())
    }
}
