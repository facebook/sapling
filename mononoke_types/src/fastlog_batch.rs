// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{
    blob::{Blob, BlobstoreValue, FastlogBatchBlob},
    errors::*,
    thrift,
    typed_hash::{ChangesetId, FastlogBatchId, FastlogBatchIdContext},
};
use blobstore::{Blobstore, Loadable, Storable};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::chain::ChainExt;
use failure_ext::Error;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use itertools::Itertools;
use rust_thrift::compact_protocol;
use std::collections::VecDeque;
use std::iter::FromIterator;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentOffset(i32);

impl ParentOffset {
    pub fn new(offset: i32) -> Self {
        Self(offset)
    }

    pub fn num(&self) -> i32 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FastlogBatch {
    latest: VecDeque<(ChangesetId, Vec<ParentOffset>)>,
    previous_batches: VecDeque<FastlogBatchId>,
}

pub const MAX_LATEST_LEN: usize = 10;
pub const MAX_BATCHES: usize = 10;

pub fn max_entries_in_fastlog_batch() -> usize {
    MAX_BATCHES * MAX_LATEST_LEN + MAX_LATEST_LEN
}

impl FastlogBatch {
    pub fn new_from_raw_list<I: IntoIterator<Item = (ChangesetId, Vec<ParentOffset>)>>(
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
        raw_list: I,
    ) -> impl Future<Item = FastlogBatch, Error = Error> {
        let chunks = raw_list
            .into_iter()
            .take(max_entries_in_fastlog_batch())
            .chunks(MAX_LATEST_LEN);
        let chunks: Vec<_> = chunks.into_iter().map(VecDeque::from_iter).collect();
        let mut chunks = chunks.into_iter();
        let latest = chunks.next().unwrap_or(VecDeque::new());

        let previous_batches = future::join_all(chunks.map(move |chunk| {
            FastlogBatch::new(VecDeque::from_iter(chunk), VecDeque::new())
                .into_blob()
                .store(ctx.clone(), &blobstore)
        }));

        previous_batches.map(|previous_batches| FastlogBatch {
            latest,
            previous_batches: VecDeque::from(previous_batches),
        })
    }

    // Prepending a child with a single parent is a special case - we only need to prepend one entry
    // with ParentOffset(1).
    pub fn prepend_child_with_single_parent(
        &self,
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
        cs_id: ChangesetId,
    ) -> impl Future<Item = FastlogBatch, Error = Error> {
        let mut new_batch = self.clone();
        if new_batch.latest.len() >= MAX_LATEST_LEN {
            let previous_latest = std::mem::replace(&mut new_batch.latest, VecDeque::new());
            let new_previous_batch = FastlogBatch::new(previous_latest, VecDeque::new());
            new_previous_batch
                .into_blob()
                .store(ctx.clone(), &blobstore)
                .map(move |new_batch_id| {
                    if new_batch.previous_batches.len() >= MAX_BATCHES {
                        new_batch.previous_batches.pop_back();
                    }
                    new_batch.latest.push_front((cs_id, vec![ParentOffset(1)]));
                    new_batch.previous_batches.push_front(new_batch_id);
                    new_batch
                })
                .left_future()
        } else {
            new_batch.latest.push_front((cs_id, vec![ParentOffset(1)]));
            future::ok(new_batch).right_future()
        }
    }

    pub fn fetch_raw_list(
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

    pub fn latest(&self) -> &VecDeque<(ChangesetId, Vec<ParentOffset>)> {
        &self.latest
    }

    pub fn previous_batches(&self) -> &VecDeque<FastlogBatchId> {
        &self.previous_batches
    }

    pub fn from_bytes(serialized: &Bytes) -> Result<FastlogBatch> {
        let thrift_entry: ::std::result::Result<thrift::FastlogBatch, Error> =
            compact_protocol::deserialize(serialized)
                .map_err(|err| ErrorKind::BlobDeserializeError(format!("{}", err)).into());
        thrift_entry.and_then(Self::from_thrift)
    }

    pub fn from_thrift(th: thrift::FastlogBatch) -> Result<FastlogBatch> {
        let latest: Result<VecDeque<_>> = th
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

        let previous_batches: Result<VecDeque<_>> = th
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

    pub fn into_bytes(self) -> Bytes {
        compact_protocol::serialize(&self.into_thrift())
    }

    pub fn into_thrift(self) -> thrift::FastlogBatch {
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

    fn new(
        latest: VecDeque<(ChangesetId, Vec<ParentOffset>)>,
        previous_batches: VecDeque<FastlogBatchId>,
    ) -> Self {
        Self {
            latest,
            previous_batches,
        }
    }
}

impl BlobstoreValue for FastlogBatch {
    type Key = FastlogBatchId;

    fn into_blob(self) -> FastlogBatchBlob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = FastlogBatchIdContext::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data)
    }

    fn from_blob(blob: FastlogBatchBlob) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("FastlogBatch".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::hash::Blake2;
    use context::CoreContext;
    use fixtures::linear;
    use pretty_assertions::assert_eq;
    use quickcheck::{quickcheck, TestResult};
    use tokio::runtime::Runtime;

    #[test]
    fn test_fastlog_batch_empty() -> Result<()> {
        let mut rt = Runtime::new().unwrap();
        let blobstore = Arc::new(linear::getrepo().get_blobstore());
        let ctx = CoreContext::test_mock();

        let list = VecDeque::new();
        let f = FastlogBatch::new_from_raw_list(ctx, blobstore, list);
        let batch = rt.block_on(f)?;
        assert!(batch.latest.is_empty());
        assert!(batch.previous_batches.is_empty());

        Ok(())
    }

    #[test]
    fn test_fastlog_batch_single() -> Result<()> {
        let mut rt = Runtime::new().unwrap();
        let blobstore = Arc::new(linear::getrepo().get_blobstore());
        let ctx = CoreContext::test_mock();

        let mut list = VecDeque::new();
        let csid = ChangesetId::new(Blake2::from_byte_array([1; 32]));
        list.push_back((csid, vec![]));
        let f = FastlogBatch::new_from_raw_list(ctx, blobstore, list.clone());
        let batch = rt.block_on(f)?;
        assert_eq!(batch.latest, list);
        assert!(batch.previous_batches.is_empty());

        Ok(())
    }

    #[test]
    fn test_fastlog_batch_large() -> Result<()> {
        let mut rt = Runtime::new().unwrap();
        let blobstore = Arc::new(linear::getrepo().get_blobstore());
        let ctx = CoreContext::test_mock();

        let mut list = VecDeque::new();
        for i in 0..max_entries_in_fastlog_batch() {
            let csid = ChangesetId::new(Blake2::from_byte_array([i as u8; 32]));
            list.push_back((csid, vec![ParentOffset(1)]));
        }

        let f = FastlogBatch::new_from_raw_list(ctx.clone(), blobstore.clone(), list.clone());
        let batch = rt.block_on(f)?;
        assert_eq!(batch.latest.len(), MAX_LATEST_LEN);
        assert_eq!(batch.previous_batches.len(), MAX_BATCHES);

        let fetched_list = rt.block_on(batch.fetch_raw_list(ctx, blobstore))?;

        assert_eq!(fetched_list, Vec::from(list));
        Ok(())
    }

    #[test]
    fn test_fastlog_batch_overflow() -> Result<()> {
        let mut rt = Runtime::new().unwrap();
        let blobstore = Arc::new(linear::getrepo().get_blobstore());
        let ctx = CoreContext::test_mock();

        let mut list = VecDeque::new();
        for i in 0..max_entries_in_fastlog_batch() + 1 {
            let csid = ChangesetId::new(Blake2::from_byte_array([i as u8; 32]));
            list.push_back((csid, vec![ParentOffset(1)]));
        }

        let f = FastlogBatch::new_from_raw_list(ctx.clone(), blobstore.clone(), list.clone());
        let batch = rt.block_on(f)?;
        assert_eq!(batch.latest.len(), MAX_LATEST_LEN);
        assert_eq!(batch.previous_batches.len(), MAX_BATCHES);

        let fetched_list = rt.block_on(batch.fetch_raw_list(ctx, blobstore))?;

        list.pop_back();
        assert_eq!(fetched_list, Vec::from(list));
        Ok(())
    }

    quickcheck! {
        fn fastlog_roundtrip(hashes: Vec<(ChangesetId, i32)>) -> TestResult {
            let mut rt = Runtime::new().unwrap();
            let blobstore = Arc::new(linear::getrepo().get_blobstore());
            let ctx = CoreContext::test_mock();

            let mut raw_list = VecDeque::new();
            for (cs_id, offset) in hashes {
                raw_list.push_back((cs_id, vec![ParentOffset(offset)]));
            }

            let f = FastlogBatch::new_from_raw_list(
                ctx.clone(),
                blobstore.clone(),
                raw_list.clone(),
            );
            let batch = rt.block_on(f).unwrap();

            if batch.latest.len() > MAX_LATEST_LEN {
                return TestResult::from_bool(false);
            }
            if batch.previous_batches.len() > MAX_BATCHES {
                return TestResult::from_bool(false);
            }

            raw_list.truncate(max_entries_in_fastlog_batch());
            let actual = rt.block_on(batch.fetch_raw_list(ctx, blobstore)).unwrap();

            TestResult::from_bool(actual == Vec::from(raw_list))
        }
    }
}
