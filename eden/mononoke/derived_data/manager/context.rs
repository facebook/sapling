/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use cacheblob::MemWritesBlobstore;
use context::CoreContext;
use filenodes::Filenodes;
use futures::future::try_join_all;
use metaconfig_types::DerivedDataTypesConfig;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::derivable::BonsaiDerivable;
use crate::manager::derive::Rederivation;

/// Context for performing derivation.
///
/// This struct is passed to derivation implementations.  They can use it
/// to access repository attributes or request access to dependent
/// derived data types.
#[derive(Clone)]
pub struct DerivationContext {
    pub(crate) bonsai_hg_mapping: Option<Arc<dyn BonsaiHgMapping>>,
    bonsai_git_mapping: Option<Arc<dyn BonsaiGitMapping>>,
    pub(crate) filenodes: Option<Arc<dyn Filenodes>>,
    config_name: String,
    config: DerivedDataTypesConfig,
    rederivation: Option<Arc<dyn Rederivation>>,
    pub(crate) blobstore: Arc<dyn Blobstore>,

    /// Write cache layered over the blobstore.  This is the same object
    /// with two views, so we can return a reference to the `Arc<dyn
    /// Blobstore>` version if needed.
    blobstore_write_cache: Option<(
        Arc<dyn Blobstore>,
        Arc<MemWritesBlobstore<Arc<dyn Blobstore>>>,
    )>,
}

impl DerivationContext {
    pub(crate) fn new(
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
        filenodes: Arc<dyn Filenodes>,
        config_name: String,
        config: DerivedDataTypesConfig,
        blobstore: Arc<dyn Blobstore>,
    ) -> Self {
        // Start with None. Use with_rederivation later if needed
        let rederivation = None;
        DerivationContext {
            bonsai_hg_mapping: Some(bonsai_hg_mapping),
            bonsai_git_mapping: Some(bonsai_git_mapping),
            filenodes: Some(filenodes),
            config_name,
            config,
            rederivation,
            blobstore,
            blobstore_write_cache: None,
        }
    }

    pub(crate) fn with_replaced_rederivation(
        &self,
        rederivation: Option<Arc<dyn Rederivation>>,
    ) -> Self {
        Self {
            rederivation,
            ..self.clone()
        }
    }

    // For dangerous-override: allow replacement of bonsai-hg-mapping
    pub fn with_replaced_bonsai_hg_mapping(
        &self,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    ) -> Self {
        Self {
            bonsai_hg_mapping: Some(bonsai_hg_mapping),
            ..self.clone()
        }
    }

    // For dangerous-override: allow replacement of filenodes
    pub fn with_replaced_filenodes(&self, filenodes: Arc<dyn Filenodes>) -> Self {
        Self {
            filenodes: Some(filenodes),
            ..self.clone()
        }
    }

    pub fn with_replaced_config(
        &self,
        config_name: String,
        config: DerivedDataTypesConfig,
    ) -> Self {
        Self {
            config_name,
            config,
            ..self.clone()
        }
    }

    pub fn with_replaced_blobstore(&self, blobstore: Arc<dyn Blobstore>) -> Self {
        Self {
            blobstore,
            ..self.clone()
        }
    }

    /// Fetch previously derived data.
    pub async fn fetch_derived<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
    ) -> Result<Option<Derivable>>
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(rederivation) = self.rederivation.as_ref() {
            if rederivation.needs_rederive(Derivable::VARIANT, csid) == Some(true) {
                return Ok(None);
            }
        }
        let derived = Derivable::fetch(ctx, self, csid).await?;
        Ok(derived)
    }

    /// Fetch a batch of previously derived data.
    pub async fn fetch_derived_batch<Derivable>(
        &self,
        ctx: &CoreContext,
        mut csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Derivable>>
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(rederivation) = self.rederivation.as_ref() {
            csids.retain(|csid| {
                rederivation.needs_rederive(Derivable::VARIANT, *csid) != Some(true)
            });
        }
        let derived = Derivable::fetch_batch(ctx, self, &csids).await?;
        Ok(derived)
    }

    /// Fetch previously derived data that is known to be derived as it is a
    /// dependency of the current changeset.
    pub async fn fetch_dependency<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
    ) -> Result<Derivable>
    where
        Derivable: BonsaiDerivable,
    {
        self.fetch_derived(ctx, csid).await?.ok_or_else(|| {
            anyhow!(
                "dependency '{}' of {} was not already derived",
                Derivable::NAME,
                csid
            )
        })
    }

    /// Fetch a dependency for all parents of a changeset.
    pub async fn fetch_parents<Derivable>(
        &self,
        ctx: &CoreContext,
        bonsai: &BonsaiChangeset,
    ) -> Result<Vec<Derivable>>
    where
        Derivable: BonsaiDerivable,
    {
        self.fetch_unknown_parents(ctx, None, bonsai).await
    }

    /// Fetch a dependency for all parents of a changeset if the dependency is
    /// not already known.
    pub async fn fetch_unknown_parents<Derivable>(
        &self,
        ctx: &CoreContext,
        known: Option<&HashMap<ChangesetId, Derivable>>,
        bonsai: &BonsaiChangeset,
    ) -> Result<Vec<Derivable>>
    where
        Derivable: BonsaiDerivable,
    {
        try_join_all(bonsai.parents().map(|p| async move {
            self.fetch_unknown_dependency(ctx, known, p)
                .await
                .with_context(|| {
                    format!(
                        "could not fetch '{}' for parents of {}",
                        Derivable::NAME,
                        bonsai.get_changeset_id(),
                    )
                })
        }))
        .await
    }

    /// Fetch derived data value for changeset if it is not already known.
    pub async fn fetch_unknown_dependency<Derivable>(
        &self,
        ctx: &CoreContext,
        known: Option<&HashMap<ChangesetId, Derivable>>,
        csid: ChangesetId,
    ) -> Result<Derivable>
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(parent) = known.and_then(|k| k.get(&csid)) {
            return Ok(parent.clone());
        }
        self.fetch_dependency(ctx, csid).await
    }

    /// The blobstore that should be used for storing and retrieving blobs.
    pub fn blobstore(&self) -> &Arc<dyn Blobstore> {
        match &self.blobstore_write_cache {
            Some((blobstore, _)) => blobstore,
            None => &self.blobstore,
        }
    }

    pub fn bonsai_hg_mapping(&self) -> Result<&dyn BonsaiHgMapping> {
        self.bonsai_hg_mapping
            .as_deref()
            .context("Missing BonsaiHgMapping")
    }

    pub fn bonsai_git_mapping(&self) -> Result<&dyn BonsaiGitMapping> {
        self.bonsai_git_mapping
            .as_deref()
            .context("Missing BonsaiGitMapping")
    }

    pub fn filenodes(&self) -> Result<&dyn Filenodes> {
        self.filenodes.as_deref().context("Missing filenodes")
    }

    pub fn config_name(&self) -> String {
        self.config_name.clone()
    }

    /// The config that should be used for derivation.
    pub fn config(&self) -> &DerivedDataTypesConfig {
        &self.config
    }

    /// Mapping key prefix for a particular derived data type.
    pub fn mapping_key_prefix<Derivable>(&self) -> &str
    where
        Derivable: BonsaiDerivable,
    {
        self.config()
            .mapping_key_prefixes
            .get(&Derivable::VARIANT)
            .map_or("", String::as_str)
    }

    pub(crate) fn needs_rederive<Derivable>(&self, csid: ChangesetId) -> bool
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(rederivation) = self.rederivation.as_ref() {
            rederivation.needs_rederive(Derivable::VARIANT, csid) == Some(true)
        } else {
            false
        }
    }

    pub(crate) fn mark_derived<Derivable>(&self, csid: ChangesetId)
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(rederivation) = self.rederivation.as_ref() {
            rederivation.mark_derived(Derivable::VARIANT, csid);
        }
    }

    /// Enable write batching for this derivation context.
    ///
    /// With write batching enabled, blobstore writes are sent to a write
    /// cache, rather than directly to the blobstore.  They must be flushed by
    /// a call to `flush` to make them persistent.
    pub(crate) fn enable_write_batching(&mut self) {
        if self.blobstore_write_cache.is_none() {
            let blobstore = Arc::new(MemWritesBlobstore::new(self.blobstore.clone()));
            self.blobstore_write_cache = Some((blobstore.clone(), blobstore));
        }
    }

    /// Flush any pending writes for this derivation context.
    pub(crate) async fn flush(&self, ctx: &CoreContext) -> Result<()> {
        if let Some((_, blobstore)) = &self.blobstore_write_cache {
            blobstore.persist(ctx).await?;
        }
        Ok(())
    }
}
