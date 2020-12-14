/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingEntry};
use context::CoreContext;
use futures::compat::Future01CompatExt;
use mercurial_types::HgChangesetId;
use mononoke_types::{BonsaiChangeset, ChangesetId, RepositoryId};

use std::{collections::HashMap, sync::Arc};

use derived_data::{BonsaiDerivable, BonsaiDerived, BonsaiDerivedMapping, DeriveError};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MappedHgChangesetId(pub HgChangesetId);

#[async_trait]
impl BonsaiDerivable for MappedHgChangesetId {
    const NAME: &'static str = "hgchangesets";

    type Options = ();

    async fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        _options: &Self::Options,
    ) -> Result<Self, Error> {
        crate::derive_hg_changeset::derive_from_parents(ctx, repo, bonsai, parents).await
    }
}

#[derive(Clone)]
pub struct HgChangesetIdMapping {
    repo_id: RepositoryId,
    mapping: Arc<dyn BonsaiHgMapping>,
}

impl HgChangesetIdMapping {
    pub fn new(repo: &BlobRepo) -> Self {
        Self {
            repo_id: repo.get_repoid(),
            mapping: repo.attribute_expected::<dyn BonsaiHgMapping>().clone(),
        }
    }
}

#[async_trait]
impl BonsaiDerivedMapping for HgChangesetIdMapping {
    type Value = MappedHgChangesetId;

    async fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>, Error> {
        let map = self
            .mapping
            .get(ctx, self.repo_id, csids.into())
            .compat()
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, MappedHgChangesetId(entry.hg_cs_id)))
            .collect();
        Ok(map)
    }

    async fn put(&self, ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> Result<(), Error> {
        self.mapping
            .add(
                ctx,
                BonsaiHgMappingEntry {
                    repo_id: self.repo_id,
                    hg_cs_id: id.0,
                    bcs_id: csid,
                },
            )
            .compat()
            .await?;
        Ok(())
    }

    fn options(&self) {}
}

#[async_trait]
impl BonsaiDerived for MappedHgChangesetId {
    type DefaultMapping = HgChangesetIdMapping;

    fn default_mapping(
        _ctx: &CoreContext,
        repo: &BlobRepo,
    ) -> Result<Self::DefaultMapping, DeriveError> {
        Ok(HgChangesetIdMapping::new(repo))
    }
}
