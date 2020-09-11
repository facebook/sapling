/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingEntry};
use context::CoreContext;
use futures::{FutureExt, TryFutureExt};
use futures_ext::{BoxFuture, FutureExt as _};
use futures_old::Future;
use mercurial_types::HgChangesetId;
use mononoke_types::{BonsaiChangeset, ChangesetId, RepositoryId};

use std::{collections::HashMap, sync::Arc};

use derived_data::{BonsaiDerived, BonsaiDerivedMapping};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MappedHgChangesetId(pub HgChangesetId);

impl BonsaiDerived for MappedHgChangesetId {
    const NAME: &'static str = "hgchangesets";
    type Mapping = HgChangesetIdMapping;

    fn mapping(_ctx: &CoreContext, repo: &BlobRepo) -> Self::Mapping {
        HgChangesetIdMapping::new(repo)
    }

    fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> BoxFuture<Self, Error> {
        crate::derive_hg_changeset::derive_from_parents(ctx, repo, bonsai, parents)
            .boxed()
            .compat()
            .boxify()
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

impl BonsaiDerivedMapping for HgChangesetIdMapping {
    type Value = MappedHgChangesetId;

    fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        self.mapping
            .get(ctx, self.repo_id, csids.into())
            .map(|v| {
                v.into_iter()
                    .map(|entry| (entry.bcs_id, MappedHgChangesetId(entry.hg_cs_id)))
                    .collect()
            })
            .boxify()
    }

    fn put(&self, ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> BoxFuture<(), Error> {
        self.mapping
            .add(
                ctx,
                BonsaiHgMappingEntry {
                    repo_id: self.repo_id,
                    hg_cs_id: id.0,
                    bcs_id: csid,
                },
            )
            .map(|_| ())
            .boxify()
    }
}
