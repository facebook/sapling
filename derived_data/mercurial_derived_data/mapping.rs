/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobrepo::BlobRepo;
use bonsai_hg_mapping::BonsaiHgMapping;
use context::CoreContext;
use failure::Error;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::HgChangesetId;
use mononoke_types::{BonsaiChangeset, ChangesetId, RepositoryId};

use std::{collections::HashMap, sync::Arc};

use derived_data::{BonsaiDerived, BonsaiDerivedMapping};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MappedHgChangesetId(HgChangesetId);

impl BonsaiDerived for MappedHgChangesetId {
    const NAME: &'static str = "hgchangesets";

    fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> BoxFuture<Self, Error> {
        let bcs_id = bonsai.get_changeset_id();
        repo.get_hg_from_bonsai_changeset(ctx, bcs_id)
            .map(|hg_cs_id| MappedHgChangesetId(hg_cs_id))
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
            mapping: repo.get_bonsai_hg_mapping(),
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

    // This just succeeds, because generation of the derived data also saves the mapping
    fn put(&self, _ctx: CoreContext, _csid: ChangesetId, _id: Self::Value) -> BoxFuture<(), Error> {
        future::ok(()).boxify()
    }
}
