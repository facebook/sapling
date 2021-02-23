/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{format_err, Context, Result};
use async_trait::async_trait;
use slog::{debug, info};

use dag::{InProcessIdDag, Location};

use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};

use crate::bundle::SqlBundleStore;
use crate::dag::Dag;
use crate::iddag::IdDagSaveStore;
use crate::idmap::{
    CacheHandlers, CachedIdMap, ConcurrentMemIdMap, IdMap, OverlayIdMap, SqlIdMapFactory,
};
use crate::logging::log_new_bundle;
use crate::types::{DagBundle, IdMapVersion};
use crate::{CloneData, SegmentedChangelog, StreamCloneData};

pub struct SegmentedChangelogManager {
    repo_id: RepositoryId,
    bundle_store: SqlBundleStore,
    iddag_save_store: IdDagSaveStore,
    idmap_factory: SqlIdMapFactory,
    cache_handlers: Option<CacheHandlers>,
    with_in_memory_write_idmap: bool,
}

impl SegmentedChangelogManager {
    pub fn new(
        repo_id: RepositoryId,
        bundle_store: SqlBundleStore,
        iddag_save_store: IdDagSaveStore,
        idmap_factory: SqlIdMapFactory,
        cache_handlers: Option<CacheHandlers>,
        with_in_memory_write_idmap: bool,
    ) -> Self {
        Self {
            repo_id,
            bundle_store,
            iddag_save_store,
            idmap_factory,
            cache_handlers,
            with_in_memory_write_idmap,
        }
    }

    pub async fn save_dag(
        &self,
        ctx: &CoreContext,
        iddag: &InProcessIdDag,
        idmap_version: IdMapVersion,
    ) -> Result<DagBundle> {
        // Save the IdDag
        let iddag_version = self
            .iddag_save_store
            .save(&ctx, &iddag)
            .await
            .with_context(|| format!("repo {}: error saving iddag", self.repo_id))?;
        // Update BundleStore
        let bundle = DagBundle::new(iddag_version, idmap_version);
        self.bundle_store
            .set(&ctx, bundle)
            .await
            .with_context(|| format!("repo {}: error updating bundle store", self.repo_id))?;
        log_new_bundle(ctx, self.repo_id, bundle);
        info!(
            ctx.logger(),
            "repo {}: segmented changelog dag bundle saved, idmap_version: {}, iddag_version: {}",
            self.repo_id,
            idmap_version,
            iddag_version,
        );
        Ok(bundle)
    }

    pub async fn load_dag(&self, ctx: &CoreContext) -> Result<(DagBundle, Dag)> {
        let bundle = self
            .bundle_store
            .get(&ctx)
            .await
            .with_context(|| {
                format!(
                    "repo {}: error loading segmented changelog bundle data",
                    self.repo_id
                )
            })?
            .ok_or_else(|| {
                format_err!(
                    "repo {}: segmented changelog metadata not found, maybe repo is not seeded",
                    self.repo_id
                )
            })?;
        let iddag = self
            .iddag_save_store
            .load(&ctx, bundle.iddag_version)
            .await
            .with_context(|| format!("repo {}: failed to load iddag", self.repo_id))?;
        let idmap = self.new_idmap(bundle.idmap_version);
        debug!(
            ctx.logger(),
            "segmented changelog dag successfully loaded - repo_id: {}, idmap_version: {}, \
            iddag_version: {} ",
            self.repo_id,
            bundle.idmap_version,
            bundle.iddag_version,
        );
        let dag = Dag::new(iddag, idmap);
        Ok((bundle, dag))
    }

    pub fn new_idmap(&self, idmap_version: IdMapVersion) -> Arc<dyn IdMap> {
        let mut idmap: Arc<dyn IdMap> = Arc::new(self.idmap_factory.sql_idmap(idmap_version));
        if let Some(cache_handlers) = &self.cache_handlers {
            idmap = Arc::new(CachedIdMap::new(
                idmap,
                cache_handlers.clone(),
                self.repo_id,
                idmap_version,
            ));
        }
        if self.with_in_memory_write_idmap {
            idmap = Arc::new(OverlayIdMap::new(
                Arc::new(ConcurrentMemIdMap::new()),
                idmap,
            ));
        }
        idmap
    }
}

#[async_trait]
impl SegmentedChangelog for SegmentedChangelogManager {
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        location: Location<ChangesetId>,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        let (_, dag) = self.load_dag(&ctx).await.with_context(|| {
            format!(
                "repo {}: error loading segmented changelog from save",
                self.repo_id
            )
        })?;
        dag.location_to_many_changeset_ids(ctx, location, count)
            .await
    }

    async fn many_changeset_ids_to_locations(
        &self,
        ctx: &CoreContext,
        client_head: ChangesetId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Location<ChangesetId>>> {
        let (_, dag) = self.load_dag(&ctx).await.with_context(|| {
            format!(
                "repo {}: error loading segmented changelog from save",
                self.repo_id
            )
        })?;
        dag.many_changeset_ids_to_locations(ctx, client_head, cs_ids)
            .await
    }

    async fn clone_data(&self, ctx: &CoreContext) -> Result<CloneData<ChangesetId>> {
        let (_, dag) = self.load_dag(&ctx).await.with_context(|| {
            format!(
                "repo {}: error loading segmented changelog from save",
                self.repo_id
            )
        })?;
        dag.clone_data(ctx).await
    }

    async fn full_idmap_clone_data(
        &self,
        ctx: &CoreContext,
    ) -> Result<StreamCloneData<ChangesetId>> {
        let (_, dag) = self
            .load_dag(&ctx)
            .await
            .context("error loading segmented changelog from save")?;
        dag.full_idmap_clone_data(ctx).await
    }
}
