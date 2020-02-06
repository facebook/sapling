/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use blame::{BlameRoot, BlameRootMapping};
use blobrepo::{BlobRepo, DangerousOverride};
use blobstore::Blobstore;
use cacheblob::{dummy::DummyLease, LeaseOps, MemWritesBlobstore};
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::{RootDeletedManifestId, RootDeletedManifestMapping};
use derived_data::{BonsaiDerived, BonsaiDerivedMapping, RegenerateMapping};
use derived_data_filenodes::{FilenodesOnlyPublic, FilenodesOnlyPublicMapping};
use fastlog::{RootFastlog, RootFastlogMapping};
use fsnodes::{RootFsnodeId, RootFsnodeMapping};
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_derived_data::{HgChangesetIdMapping, MappedHgChangesetId};
use mononoke_types::ChangesetId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

pub const POSSIBLE_DERIVED_TYPES: &[&str] = &[
    RootUnodeManifestId::NAME,
    RootFastlog::NAME,
    MappedHgChangesetId::NAME,
    RootFsnodeId::NAME,
    BlameRoot::NAME,
    RootDeletedManifestId::NAME,
    FilenodesOnlyPublic::NAME,
];

pub trait DerivedUtils: Send + Sync + 'static {
    /// Derive data for changeset
    fn derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<String, Error>;

    fn derive_batch(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<(), Error>;

    /// Find pending changeset (changesets for which data have not been derived)
    fn pending(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error>;

    /// Regenerate derived data for specified set of commits
    fn regenerate(&self, csids: &Vec<ChangesetId>);

    /// Get a name for this type of derived data
    fn name(&self) -> &'static str;
}

#[derive(Clone)]
struct DerivedUtilsFromMapping<M> {
    mapping: RegenerateMapping<M>,
}

impl<M> DerivedUtilsFromMapping<M> {
    fn new(mapping: M) -> Self {
        let mapping = RegenerateMapping::new(mapping);
        Self { mapping }
    }
}

impl<M> DerivedUtils for DerivedUtilsFromMapping<M>
where
    M: BonsaiDerivedMapping + Clone + 'static,
    M::Value: BonsaiDerived + std::fmt::Debug,
{
    fn derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<String, Error> {
        <M::Value as BonsaiDerived>::derive(ctx.clone(), repo, self.mapping.clone(), csid)
            .map(|result| format!("{:?}", result))
            .boxify()
    }

    fn derive_batch(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<(), Error> {
        let orig_mapping = self.mapping.clone();
        // With InMemoryMapping we can ensure that mapping entries are written only after
        // all corresponding blobs were successfully saved
        let in_memory_mapping = InMemoryMapping::new(self.mapping.clone());

        // Use `MemWritesBlobstore` to avoid blocking on writes to underlying blobstore.
        // `::persist` is later used to bulk write all pending data.
        let mut memblobstore = None;
        let repo = repo
            .dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>)
            .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
                let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));
                memblobstore = Some(blobstore.clone());
                blobstore
            });
        let memblobstore = memblobstore.expect("memblobstore should have been updated");

        stream::iter_ok(csids)
            .for_each({
                cloned!(ctx, in_memory_mapping, repo);
                move |csid| {
                    // create new context so each derivation would have its own trace
                    let ctx = CoreContext::new_with_logger(ctx.fb, ctx.logger().clone());

                    <M::Value as BonsaiDerived>::derive(
                        ctx.clone(),
                        repo.clone(),
                        in_memory_mapping.clone(),
                        csid,
                    )
                    .map(|_| ())
                }
            })
            .and_then({
                cloned!(ctx, memblobstore);
                move |_| memblobstore.persist(ctx)
            })
            .and_then(move |_| {
                let buffer = in_memory_mapping.into_buffer();
                let buffer = buffer.lock().unwrap();
                let mut futs = vec![];
                for (cs_id, value) in buffer.iter() {
                    futs.push(orig_mapping.put(ctx.clone(), *cs_id, value.clone()));
                }
                stream::futures_unordered(futs).for_each(|_| Ok(()))
            })
            .boxify()
    }

    fn pending(
        &self,
        ctx: CoreContext,
        _repo: BlobRepo,
        mut csids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        self.mapping
            .get(ctx, csids.clone())
            .map(move |derived| {
                csids.retain(|csid| !derived.contains_key(&csid));
                csids
            })
            .boxify()
    }

    fn regenerate(&self, csids: &Vec<ChangesetId>) {
        self.mapping.regenerate(csids.iter().copied())
    }

    fn name(&self) -> &'static str {
        M::Value::NAME
    }
}

#[derive(Clone)]
struct InMemoryMapping<M: BonsaiDerivedMapping + Clone> {
    mapping: M,
    buffer: Arc<Mutex<HashMap<ChangesetId, M::Value>>>,
}

impl<M> InMemoryMapping<M>
where
    M: BonsaiDerivedMapping + Clone,
    <M as BonsaiDerivedMapping>::Value: Clone,
{
    fn new(mapping: M) -> Self {
        Self {
            mapping,
            buffer: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn into_buffer(self) -> Arc<Mutex<HashMap<ChangesetId, M::Value>>> {
        self.buffer
    }
}

impl<M> BonsaiDerivedMapping for InMemoryMapping<M>
where
    M: BonsaiDerivedMapping + Clone,
    <M as BonsaiDerivedMapping>::Value: Clone,
{
    type Value = M::Value;

    fn get(
        &self,
        ctx: CoreContext,
        mut csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        let buffer = self.buffer.lock().unwrap();
        let mut ans = HashMap::new();
        csids.retain(|cs_id| {
            if let Some(v) = buffer.get(cs_id) {
                ans.insert(*cs_id, v.clone());
                false
            } else {
                true
            }
        });

        self.mapping
            .get(ctx, csids)
            .map(move |fetched| ans.into_iter().chain(fetched.into_iter()).collect())
            .boxify()
    }

    fn put(&self, _ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> BoxFuture<(), Error> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.insert(csid, id);
        future::ok(()).boxify()
    }
}

pub fn derived_data_utils(
    _ctx: CoreContext,
    repo: BlobRepo,
    name: impl AsRef<str>,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    match name.as_ref() {
        RootUnodeManifestId::NAME => {
            let mapping = RootUnodeManifestMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        RootFastlog::NAME => {
            let mapping = RootFastlogMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        MappedHgChangesetId::NAME => {
            let mapping = HgChangesetIdMapping::new(&repo);
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        RootFsnodeId::NAME => {
            let mapping = RootFsnodeMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        BlameRoot::NAME => {
            let mapping = BlameRootMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        RootDeletedManifestId::NAME => {
            let mapping = RootDeletedManifestMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        FilenodesOnlyPublic::NAME => {
            let mapping = FilenodesOnlyPublicMapping::new(repo);
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        name => Err(format_err!("Unsupported derived data type: {}", name)),
    }
}
