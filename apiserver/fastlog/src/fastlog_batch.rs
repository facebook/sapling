// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{thrift, ErrorKind, FastlogParent};
use blobstore::{Blobstore, BlobstoreBytes};
use cloned::cloned;
use context::CoreContext;
use failure_ext::{bail_err, Error};
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use manifest::Entry;
use mononoke_types::{hash::Blake2, ChangesetId, FileUnodeId, ManifestUnodeId};
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
            fetch_fastlog_batch(ctx.clone(), blobstore.clone(), entry)
                .and_then(move |maybe_batch| maybe_batch.ok_or(ErrorKind::NotFound(entry).into()))
        }
    }));

    f.and_then(move |parent_batches| {
        if parent_batches.len() < 2 {
            match parent_batches.get(0) {
                Some(parent_batch) => parent_batch
                    .prepend_single_parent(ctx, blobstore, linknode)
                    .left_future(),
                None => future::ok(FastlogBatch::new(linknode)).right_future(),
            }
        } else {
            // TODO(stash): handle merges as well
            unimplemented!()
        }
    })
}

pub(crate) fn fetch_fastlog_batch(
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

pub(crate) fn save_fastlog_batch(
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

#[derive(Clone)]
struct ParentOffset(i32);

#[derive(Clone)]
pub struct FastlogBatch {
    latest: VecDeque<(ChangesetId, Vec<ParentOffset>)>,
    previous_batches: VecDeque<FastlogBatchId>,
}

impl FastlogBatch {
    pub(crate) fn new(cs_id: ChangesetId) -> Self {
        let mut latest = VecDeque::new();
        latest.push_front((cs_id, vec![]));
        FastlogBatch {
            latest,
            previous_batches: VecDeque::new(),
        }
    }

    pub(crate) fn prepend_single_parent(
        &self,
        _ctx: CoreContext,
        _blobstore: Arc<dyn Blobstore>,
        cs_id: ChangesetId,
    ) -> impl Future<Item = FastlogBatch, Error = Error> {
        let mut new_batch = self.clone();
        // If there's just one parent, then the latest commit in the parent batch is the parent,
        // and offset to this parent is 1.
        new_batch.latest.push_front((cs_id, vec![ParentOffset(1)]));
        // TODO(stash): handle overflows
        future::ok(new_batch)
    }

    pub(crate) fn convert_to_list(
        &self,
        _ctx: CoreContext,
        _blobstore: Arc<dyn Blobstore>,
    ) -> impl Future<Item = Vec<(ChangesetId, Vec<FastlogParent>)>, Error = Error> {
        let mut res = vec![];
        for (index, (cs_id, parent_offsets)) in self.latest.iter().enumerate() {
            let mut batch_parents = vec![];
            for offset in parent_offsets {
                let parent_index = index + offset.0 as usize;
                let batch_parent = match self.latest.get(parent_index) {
                    Some((p_cs_id, _)) => FastlogParent::Known(*p_cs_id),
                    None => FastlogParent::Unknown,
                };
                batch_parents.push(batch_parent);
            }

            res.push((*cs_id, batch_parents));
        }

        if !self.previous_batches.is_empty() {
            // TODO(stash): handle previous_batches correctly
            unimplemented!()
        }

        future::ok(res)
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
    fn convert_to_list_simple() {
        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo();
        let mut rt = Runtime::new().unwrap();
        let batch = FastlogBatch::new(ONES_CSID);
        let blobstore = Arc::new(repo.get_blobstore());

        assert_eq!(
            vec![(ONES_CSID, vec![])],
            rt.block_on(batch.convert_to_list(ctx, blobstore)).unwrap()
        );
    }

    #[test]
    fn convert_to_list_prepend() {
        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo();
        let mut rt = Runtime::new().unwrap();
        let batch = FastlogBatch::new(ONES_CSID);
        let blobstore = Arc::new(repo.get_blobstore());

        assert_eq!(
            vec![(ONES_CSID, vec![])],
            rt.block_on(batch.convert_to_list(ctx.clone(), blobstore.clone()))
                .unwrap()
        );

        let prepended = rt
            .block_on(batch.prepend_single_parent(ctx.clone(), blobstore.clone(), TWOS_CSID))
            .unwrap();
        assert_eq!(
            vec![
                (TWOS_CSID, vec![FastlogParent::Known(ONES_CSID)]),
                (ONES_CSID, vec![])
            ],
            rt.block_on(prepended.convert_to_list(ctx.clone(), blobstore.clone()))
                .unwrap()
        );

        let prepended = rt
            .block_on(prepended.prepend_single_parent(ctx.clone(), blobstore.clone(), THREES_CSID))
            .unwrap();
        assert_eq!(
            vec![
                (THREES_CSID, vec![FastlogParent::Known(TWOS_CSID)]),
                (TWOS_CSID, vec![FastlogParent::Known(ONES_CSID)]),
                (ONES_CSID, vec![])
            ],
            rt.block_on(prepended.convert_to_list(ctx, blobstore))
                .unwrap()
        );
    }
}
