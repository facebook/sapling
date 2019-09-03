// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{thrift, ErrorKind, FastlogParent};
use blobstore::{Blobstore, BlobstoreBytes, Loadable, LoadableError, Storable};
use cloned::cloned;
use context::CoreContext;
use failure_ext::{bail_err, Error};
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use manifest::Entry;
use mononoke_types::{
    hash::{Blake2, Context},
    ChangesetId, FileUnodeId, ManifestUnodeId,
};
use rust_thrift::compact_protocol;
use std::collections::VecDeque;
use std::sync::Arc;

pub(crate) fn create_new_batch(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    unode_parents: Vec<Entry<ManifestUnodeId, FileUnodeId>>,
    linknode: ChangesetId,
) -> impl Future<Item = FastlogBatch, Error = Error> {
    let f = future::join_all(unode_parents.clone().into_iter().map({
        cloned!(ctx, blobstore);
        move |entry| {
            fetch_fastlog_batch_by_unode_id(ctx.clone(), blobstore.clone(), entry)
                .and_then(move |maybe_batch| maybe_batch.ok_or(ErrorKind::NotFound(entry).into()))
        }
    }));

    f.and_then(move |parent_batches| {
        if parent_batches.len() < 2 {
            match parent_batches.get(0) {
                Some(parent_batch) => parent_batch
                    .prepend_child_with_single_parent(ctx, blobstore, linknode)
                    .left_future(),
                None => {
                    let mut d = VecDeque::new();
                    d.push_back((linknode, vec![]));
                    future::ok(FastlogBatch::new(d)).right_future()
                }
            }
        } else {
            // TODO(stash): handle merges as well
            unimplemented!()
        }
    })
}

pub(crate) fn fetch_fastlog_batch_by_unode_id(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
) -> impl Future<Item = Option<FastlogBatch>, Error = Error> {
    let fastlog_batch_key = generate_fastlog_batch_key(unode_entry);

    blobstore
        .get(ctx, fastlog_batch_key.clone())
        .and_then(move |maybe_bytes| match maybe_bytes {
            Some(serialized) => {
                let thrift_entry: ::std::result::Result<thrift::FastlogBatch, Error> =
                    compact_protocol::deserialize(serialized.as_bytes()).map_err(|err| {
                        ErrorKind::DeserializationError(fastlog_batch_key, format!("{}", err))
                            .into()
                    });
                thrift_entry.and_then(FastlogBatch::from_thrift).map(Some)
            }
            None => Ok(None),
        })
}

pub(crate) fn save_fastlog_batch_by_unode_id(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
    batch: FastlogBatch,
) -> BoxFuture<(), Error> {
    let fastlog_batch_key = generate_fastlog_batch_key(unode_entry);

    let serialized = compact_protocol::serialize(&batch.into_thrift());
    blobstore.put(
        ctx,
        fastlog_batch_key,
        BlobstoreBytes::from_bytes(serialized),
    )
}

fn generate_fastlog_batch_key(unode_entry: Entry<ManifestUnodeId, FileUnodeId>) -> String {
    let key_part = match unode_entry {
        Entry::Leaf(file_unode_id) => format!("fileunode.{}", file_unode_id),
        Entry::Tree(mf_unode_id) => format!("manifestunode.{}", mf_unode_id),
    };
    format!("fastlogbatch.{}", key_part)
}

const MAX_LATEST_LEN: usize = 10;
const MAX_BATCHES: usize = 5;

#[derive(Clone)]
pub(crate) struct ParentOffset(i32);

#[derive(Clone)]
pub struct FastlogBatch {
    latest: VecDeque<(ChangesetId, Vec<ParentOffset>)>,
    previous_batches: VecDeque<FastlogBatchId>,
}

impl FastlogBatch {
    fn new(latest: VecDeque<(ChangesetId, Vec<ParentOffset>)>) -> Self {
        Self {
            latest,
            previous_batches: VecDeque::new(),
        }
    }

    // Prepending a child with a single parent is a special case - we only need to prepend one entry
    // with ParentOffset(1).
    pub(crate) fn prepend_child_with_single_parent(
        &self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
        cs_id: ChangesetId,
    ) -> impl Future<Item = FastlogBatch, Error = Error> {
        let new_entry = (cs_id, vec![ParentOffset(1)]);

        let mut new_batch = self.clone();
        if new_batch.latest.len() >= MAX_LATEST_LEN {
            let previous_latest = std::mem::replace(&mut new_batch.latest, VecDeque::new());
            let new_previous_batch = FastlogBatch::new(previous_latest);
            new_previous_batch
                .store(ctx.clone(), &blobstore)
                .map(move |new_batch_id| {
                    if new_batch.previous_batches.len() >= MAX_BATCHES {
                        new_batch.previous_batches.pop_back();
                    }
                    new_batch.latest.push_front(new_entry);
                    new_batch.previous_batches.push_front(new_batch_id);
                    new_batch
                })
                .left_future()
        } else {
            new_batch.latest.push_front(new_entry);
            future::ok(new_batch).right_future()
        }
    }

    pub(crate) fn fetch_flattened(
        &self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
    ) -> impl Future<Item = Vec<(ChangesetId, Vec<FastlogParent>)>, Error = Error> {
        self.fetch_raw_list(ctx, blobstore).map(|full_batch| {
            let mut res = vec![];
            for (index, (cs_id, parent_offsets)) in full_batch.iter().enumerate() {
                let mut batch_parents = vec![];
                for offset in parent_offsets {
                    let parent_index = index + offset.0 as usize;
                    let batch_parent = match full_batch.get(parent_index) {
                        Some((p_cs_id, _)) => FastlogParent::Known(*p_cs_id),
                        None => FastlogParent::Unknown,
                    };
                    batch_parents.push(batch_parent);
                }

                res.push((*cs_id, batch_parents));
            }

            res
        })
    }

    fn fetch_raw_list(
        &self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
    ) -> BoxFuture<Vec<(ChangesetId, Vec<ParentOffset>)>, Error> {
        let mut v = vec![];
        for p in self.previous_batches.iter() {
            v.push(p.load(ctx.clone(), &blobstore).from_err().and_then({
                cloned!(ctx, blobstore);
                move |full_batch| full_batch.fetch_raw_list(ctx, blobstore)
            }));
        }

        let mut res = vec![];
        res.extend(self.latest.clone());
        future::join_all(v)
            .map(move |previous_batches| {
                for p in previous_batches {
                    res.extend(p);
                }
                res
            })
            .boxify()
    }

    #[cfg(test)]
    pub(crate) fn latest(&self) -> &VecDeque<(ChangesetId, Vec<ParentOffset>)> {
        &self.latest
    }

    #[cfg(test)]
    pub(crate) fn previous_batches(&self) -> &VecDeque<FastlogBatchId> {
        &self.previous_batches
    }

    fn from_thrift(th: thrift::FastlogBatch) -> Result<FastlogBatch, Error> {
        let latest: Result<VecDeque<_>, Error> = th
            .latest
            .into_iter()
            .map(|hash_and_parents| {
                let cs_id = ChangesetId::from_thrift(hash_and_parents.cs_id);
                let offsets = hash_and_parents
                    .parent_offsets
                    .into_iter()
                    .map(|p| ParentOffset(p.0))
                    .collect();
                cs_id.map(|cs_id| (cs_id, offsets))
            })
            .collect();
        let latest = latest?;

        let previous_batches: Result<VecDeque<_>, _> = th
            .previous_batches
            .into_iter()
            .map(FastlogBatchId::from_thrift)
            .collect();

        let previous_batches = previous_batches?;
        Ok(FastlogBatch {
            latest,
            previous_batches,
        })
    }

    fn into_thrift(self) -> thrift::FastlogBatch {
        let latest_thrift = self
            .latest
            .into_iter()
            .map(|(cs_id, offsets)| {
                let parent_offsets = offsets
                    .into_iter()
                    .map(|offset| thrift::ParentOffset(offset.0))
                    .collect();

                thrift::CompressedHashAndParents {
                    cs_id: cs_id.into_thrift(),
                    parent_offsets,
                }
            })
            .collect();

        let previous_batches = self
            .previous_batches
            .into_iter()
            .map(|previous_batch| previous_batch.into_thrift())
            .collect();
        thrift::FastlogBatch {
            latest: latest_thrift,
            previous_batches,
        }
    }
}

impl Loadable for FastlogBatchId {
    type Value = FastlogBatch;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, LoadableError> {
        let blobstore_key = blobstore_fastlog_batch_key(&self);

        blobstore
            .get(ctx, blobstore_key.clone())
            .from_err()
            .and_then(move |bytes| {
                let bytes = bytes.ok_or_else(|| LoadableError::Missing(blobstore_key.clone()))?;

                let batch: Result<thrift::FastlogBatch, LoadableError> =
                    compact_protocol::deserialize(&bytes.into_bytes()).map_err(|err| {
                        let err: Error =
                            ErrorKind::DeserializationError(blobstore_key, format!("{}", err))
                                .into();
                        LoadableError::Error(err)
                    });

                let batch = batch?;
                FastlogBatch::from_thrift(batch).map_err(LoadableError::Error)
            })
            .boxify()
    }
}

impl Storable for FastlogBatch {
    type Key = FastlogBatchId;

    fn store<B: Blobstore + Clone>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Key, Error> {
        let serialized = compact_protocol::serialize(&self.clone().into_thrift());
        let mut context = FastlogBatchContext::new();
        context.update(&serialized);
        let batch_id = context.finish();

        blobstore
            .put(
                ctx,
                blobstore_fastlog_batch_key(&batch_id),
                BlobstoreBytes::from_bytes(serialized),
            )
            .map(move |()| batch_id)
            .boxify()
    }
}

fn blobstore_fastlog_batch_key(id: &FastlogBatchId) -> String {
    format!("fastlogbatch.{}", id.0)
}

#[derive(Clone)]
pub struct FastlogBatchContext(Context);

impl FastlogBatchContext {
    /// Construct a context.
    #[inline]
    pub fn new() -> Self {
        FastlogBatchContext(Context::new("fastlogbatch".as_bytes()))
    }

    #[inline]
    pub fn update<T>(&mut self, data: T)
    where
        T: AsRef<[u8]>,
    {
        self.0.update(data)
    }

    #[inline]
    pub fn finish(self) -> FastlogBatchId {
        FastlogBatchId(self.0.finish())
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct FastlogBatchId(Blake2);

impl FastlogBatchId {
    pub fn into_thrift(self) -> thrift::FastlogBatchId {
        thrift::FastlogBatchId(thrift::IdType::Blake2(self.0.into_thrift()))
    }

    pub fn from_thrift(h: thrift::FastlogBatchId) -> Result<Self, Error> {
        // This assumes that a null hash is never serialized. This should always be the
        // case.
        match h.0 {
            thrift::IdType::Blake2(blake2) => Ok(FastlogBatchId(Blake2::from_thrift(blake2)?)),
            thrift::IdType::UnknownField(x) => bail_err!(ErrorKind::InvalidThrift(
                "FastlogBatchid".into(),
                format!("unknown id type field: {}", x)
            )),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fixtures::linear;
    use mononoke_types_mocks::changesetid::{ONES_CSID, THREES_CSID, TWOS_CSID};
    use tokio::runtime::Runtime;

    #[test]
    fn fetch_flattened_simple() {
        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo();
        let mut rt = Runtime::new().unwrap();
        let mut d = VecDeque::new();
        d.push_back((ONES_CSID, vec![]));
        let batch = FastlogBatch::new(d);
        let blobstore = Arc::new(repo.get_blobstore());

        assert_eq!(
            vec![(ONES_CSID, vec![])],
            rt.block_on(batch.fetch_flattened(ctx, blobstore)).unwrap()
        );
    }

    #[test]
    fn fetch_flattened_prepend() {
        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo();
        let mut rt = Runtime::new().unwrap();
        let mut d = VecDeque::new();
        d.push_back((ONES_CSID, vec![]));
        let batch = FastlogBatch::new(d);
        let blobstore = Arc::new(repo.get_blobstore());

        assert_eq!(
            vec![(ONES_CSID, vec![])],
            rt.block_on(batch.fetch_flattened(ctx.clone(), blobstore.clone()))
                .unwrap()
        );

        let prepended = rt
            .block_on(batch.prepend_child_with_single_parent(
                ctx.clone(),
                blobstore.clone(),
                TWOS_CSID,
            ))
            .unwrap();
        assert_eq!(
            vec![
                (TWOS_CSID, vec![FastlogParent::Known(ONES_CSID)]),
                (ONES_CSID, vec![])
            ],
            rt.block_on(prepended.fetch_flattened(ctx.clone(), blobstore.clone()))
                .unwrap()
        );

        let prepended = rt
            .block_on(prepended.prepend_child_with_single_parent(
                ctx.clone(),
                blobstore.clone(),
                THREES_CSID,
            ))
            .unwrap();
        assert_eq!(
            vec![
                (THREES_CSID, vec![FastlogParent::Known(TWOS_CSID)]),
                (TWOS_CSID, vec![FastlogParent::Known(ONES_CSID)]),
                (ONES_CSID, vec![])
            ],
            rt.block_on(prepended.fetch_flattened(ctx, blobstore))
                .unwrap()
        );
    }
}
