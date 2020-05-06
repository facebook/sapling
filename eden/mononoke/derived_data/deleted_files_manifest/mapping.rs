/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::derive::{derive_deleted_files_manifest, get_changes};
use anyhow::{Error, Result};
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreGetData};
use bytes::Bytes;
use context::CoreContext;
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use futures::future::{FutureExt as NewFutureExt, TryFutureExt};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use futures_old::{
    stream::{self, FuturesUnordered},
    Future, Stream,
};
use mononoke_types::{BlobstoreBytes, BonsaiChangeset, ChangesetId, DeletedManifestId};
use repo_blobstore::RepoBlobstore;
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
    iter::FromIterator,
};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootDeletedManifestId(DeletedManifestId);

impl RootDeletedManifestId {
    pub fn deleted_manifest_id(&self) -> &DeletedManifestId {
        &self.0
    }
}

impl TryFrom<BlobstoreBytes> for RootDeletedManifestId {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        DeletedManifestId::from_bytes(&blob_bytes.into_bytes()).map(RootDeletedManifestId)
    }
}

impl TryFrom<BlobstoreGetData> for RootDeletedManifestId {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootDeletedManifestId> for BlobstoreBytes {
    fn from(root_mf_id: RootDeletedManifestId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(root_mf_id.0.blake2().as_ref()))
    }
}

impl BonsaiDerived for RootDeletedManifestId {
    const NAME: &'static str = "deleted_manifest";
    type Mapping = RootDeletedManifestMapping;

    fn mapping(_ctx: &CoreContext, repo: &BlobRepo) -> Self::Mapping {
        RootDeletedManifestMapping::new(repo.blobstore().clone())
    }

    fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> BoxFuture<Self, Error> {
        let bcs_id = bonsai.get_changeset_id();
        get_changes(ctx.clone(), repo.clone(), bonsai)
            .boxed()
            .compat()
            .and_then(move |changes| {
                derive_deleted_files_manifest(
                    ctx,
                    repo,
                    bcs_id,
                    parents
                        .into_iter()
                        .map(|root_mf_id| root_mf_id.deleted_manifest_id().clone())
                        .collect(),
                    changes,
                )
            })
            .map(RootDeletedManifestId)
            .boxify()
    }
}

#[derive(Clone)]
pub struct RootDeletedManifestMapping {
    blobstore: RepoBlobstore,
}

impl RootDeletedManifestMapping {
    pub fn new(blobstore: RepoBlobstore) -> Self {
        Self { blobstore }
    }

    fn format_key(&self, cs_id: ChangesetId) -> String {
        format!("derived_root_deleted_manifest.{}", cs_id)
    }

    fn fetch_deleted_manifest(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> impl Future<Item = Option<(ChangesetId, RootDeletedManifestId)>, Error = Error> {
        self.blobstore
            .get(ctx.clone(), self.format_key(cs_id))
            .and_then(|maybe_bytes| maybe_bytes.map(|bytes| bytes.try_into()).transpose())
            .map(move |maybe_root_mf_id| maybe_root_mf_id.map(|root_mf_id| (cs_id, root_mf_id)))
    }
}

impl BonsaiDerivedMapping for RootDeletedManifestMapping {
    type Value = RootDeletedManifestId;

    fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        let gets = csids.into_iter().map(|cs_id| {
            self.fetch_deleted_manifest(ctx.clone(), cs_id)
                .map(|maybe_root_mf_id| stream::iter_ok(maybe_root_mf_id.into_iter()))
        });
        FuturesUnordered::from_iter(gets)
            .flatten()
            .collect_to()
            .boxify()
    }

    fn put(&self, ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> BoxFuture<(), Error> {
        self.blobstore.put(ctx, self.format_key(csid), id.into())
    }
}
