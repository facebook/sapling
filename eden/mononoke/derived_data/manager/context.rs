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
use bonsai_hg_mapping::BonsaiHgMapping;
use cacheblob::MemWritesBlobstore;
use context::CoreContext;
use filenodes::Filenodes;
use futures::future::try_join_all;
use metaconfig_types::DerivedDataTypesConfig;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;

use crate::derivable::BonsaiDerivable;
use crate::manager::derive::Rederivation;
use crate::manager::DerivedDataManager;

/// Context for performing derivation.
///
/// This struct is passed to derivation implementations.  They can use it
/// to access repository attributes or request access to dependent
/// derived data types.
#[derive(Clone)]
pub struct DerivationContext {
    manager: DerivedDataManager,
    rederivation: Option<Arc<dyn Rederivation>>,
    blobstore: Arc<dyn Blobstore>,

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
        manager: DerivedDataManager,
        rederivation: Option<Arc<dyn Rederivation>>,
        blobstore: Arc<dyn Blobstore>,
    ) -> Self {
        DerivationContext {
            manager,
            rederivation,
            blobstore,
            blobstore_write_cache: None,
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
            if rederivation.needs_rederive(Derivable::NAME, csid) == Some(true) {
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
            csids.retain(|csid| rederivation.needs_rederive(Derivable::NAME, *csid) != Some(true));
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

    /// Cause derivation of a dependency, and fetch the result.
    ///
    /// In the future, this will be removed in favour of making the manager
    /// arrange for dependent derived data to always be derived before
    /// derivation starts.
    pub async fn derive_dependency<Derivable>(
        &self,
        ctx: &CoreContext,
        csid: ChangesetId,
    ) -> Result<Derivable>
    where
        Derivable: BonsaiDerivable,
    {
        Ok(self
            .manager
            .derive::<Derivable>(ctx, csid, self.rederivation.clone())
            .await?)
    }

    /// The repo id of the repo being derived.
    pub fn repo_id(&self) -> RepositoryId {
        self.manager.repo_id()
    }

    /// The repo name of the repo being derived.
    pub fn repo_name(&self) -> &str {
        self.manager.repo_name()
    }

    /// The blobstore that should be used for storing and retrieving blobs.
    pub fn blobstore(&self) -> &Arc<dyn Blobstore> {
        match &self.blobstore_write_cache {
            Some((blobstore, _)) => blobstore,
            None => &self.blobstore,
        }
    }

    pub fn bonsai_hg_mapping(&self) -> Result<&dyn BonsaiHgMapping> {
        self.manager.bonsai_hg_mapping()
    }

    pub fn filenodes(&self) -> Result<&dyn Filenodes> {
        self.manager.filenodes()
    }

    /// The config that should be used for derivation.
    pub fn config(&self) -> &DerivedDataTypesConfig {
        self.manager.config()
    }

    /// Mapping key prefix for a particular derived data type.
    pub fn mapping_key_prefix<Derivable>(&self) -> &str
    where
        Derivable: BonsaiDerivable,
    {
        self.config()
            .mapping_key_prefixes
            .get(Derivable::NAME)
            .map_or("", String::as_str)
    }

    pub(crate) fn needs_rederive<Derivable>(&self, csid: ChangesetId) -> bool
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(rederivation) = self.rederivation.as_ref() {
            rederivation.needs_rederive(Derivable::NAME, csid) == Some(true)
        } else {
            false
        }
    }

    pub(crate) fn mark_derived<Derivable>(&self, csid: ChangesetId)
    where
        Derivable: BonsaiDerivable,
    {
        if let Some(rederivation) = self.rederivation.as_ref() {
            rederivation.mark_derived(Derivable::NAME, csid);
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
