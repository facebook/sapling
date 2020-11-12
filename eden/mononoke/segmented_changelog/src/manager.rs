/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::{format_err, Context, Result};
use slog::{debug, info};

use dag::InProcessIdDag;

use context::CoreContext;
use mononoke_types::RepositoryId;

use crate::bundle::SqlBundleStore;
use crate::dag::Dag;
use crate::iddag::IdDagSaveStore;
use crate::idmap::{SqlIdMap, SqlIdMapFactory};
use crate::logging::log_new_bundle;
use crate::types::{DagBundle, IdMapVersion};

pub struct SegmentedChangelogManager {
    repo_id: RepositoryId,
    bundle_store: SqlBundleStore,
    iddag_save_store: IdDagSaveStore,
    idmap_factory: SqlIdMapFactory,
}

impl SegmentedChangelogManager {
    pub fn new(
        repo_id: RepositoryId,
        bundle_store: SqlBundleStore,
        iddag_save_store: IdDagSaveStore,
        idmap_factory: SqlIdMapFactory,
    ) -> Self {
        Self {
            repo_id,
            bundle_store,
            iddag_save_store,
            idmap_factory,
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
            .context("saving iddag")?;
        // Update BundleStore
        let bundle = DagBundle::new(iddag_version, idmap_version);
        self.bundle_store
            .set(&ctx, bundle)
            .await
            .context("updating bundle store")?;
        log_new_bundle(ctx, self.repo_id, bundle);
        info!(
            ctx.logger(),
            "segmented changelog dag bundle saved, repo_id: {}, idmap_version: {}, \
            iddag_version: {} ",
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
            .context("failed to load segmented changelog bundle data")?
            .ok_or_else(|| {
                format_err!("segmented changelog metadata not found, maybe repo is not seeded")
            })?;
        let iddag = self
            .iddag_save_store
            .load(&ctx, bundle.iddag_version)
            .await
            .context("failed to load iddag")?;
        let idmap = self.new_sql_idmap(bundle.idmap_version);
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

    pub fn new_sql_idmap(&self, idmap_version: IdMapVersion) -> Arc<SqlIdMap> {
        Arc::new(self.idmap_factory.sql_idmap(idmap_version))
    }
}
