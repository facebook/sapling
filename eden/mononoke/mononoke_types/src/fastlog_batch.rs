/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::blob::FastlogBatchBlob;
use crate::errors::ErrorKind;
use crate::thrift;
use crate::typed_hash::ChangesetId;
use crate::typed_hash::FastlogBatchId;
use crate::typed_hash::FastlogBatchIdContext;
use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::Storable;
use bytes::Bytes;
use context::CoreContext;
use fbthrift::compact_protocol;
use futures::future::try_join_all;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use itertools::Itertools;
use std::collections::VecDeque;

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
    pub async fn new_from_raw_list<'a, B: Blobstore>(
        ctx: &'a CoreContext,
        blobstore: &'a B,
        raw_list: impl IntoIterator<Item = (ChangesetId, Vec<ParentOffset>)>,
    ) -> Result<FastlogBatch> {
        let chunks = raw_list
            .into_iter()
            .take(max_entries_in_fastlog_batch())
            .chunks(MAX_LATEST_LEN);
        let chunks: Vec<_> = chunks.into_iter().map(VecDeque::from_iter).collect();
        let mut chunks = chunks.into_iter();
        let latest = chunks.next().unwrap_or_default();

        let previous_batches = VecDeque::from(
            try_join_all(chunks.map(move |chunk| {
                FastlogBatch::new(VecDeque::from_iter(chunk), VecDeque::new())
                    .into_blob()
                    .store(ctx, blobstore)
            }))
            .await?,
        );

        Ok(FastlogBatch {
            latest,
            previous_batches,
        })
    }

    // Prepending a child with a single parent is a special case - we only need to prepend one entry
    // with ParentOffset(1).
    pub async fn prepend_child_with_single_parent<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
        cs_id: ChangesetId,
    ) -> Result<FastlogBatch> {
        let mut new_batch = self.clone();
        if new_batch.latest.len() >= MAX_LATEST_LEN {
            let previous_latest = std::mem::take(&mut new_batch.latest);
            let new_previous_batch = FastlogBatch::new(previous_latest, VecDeque::new());
            let new_batch_id = new_previous_batch.into_blob().store(ctx, blobstore).await?;
            if new_batch.previous_batches.len() >= MAX_BATCHES {
                new_batch.previous_batches.pop_back();
            }
            new_batch.latest.push_front((cs_id, vec![ParentOffset(1)]));
            new_batch.previous_batches.push_front(new_batch_id);
            Ok(new_batch)
        } else {
            new_batch.latest.push_front((cs_id, vec![ParentOffset(1)]));
            Ok(new_batch)
        }
    }

    pub fn fetch_raw_list<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> BoxFuture<'a, Result<Vec<(ChangesetId, Vec<ParentOffset>)>>> {
        let mut v = vec![];
        for p in &self.previous_batches {
            v.push(async move {
                let p_load = p.load(ctx, blobstore);
                let full_batch = p_load.await?;
                full_batch.fetch_raw_list(ctx, blobstore).await
            });
        }

        async move {
            let mut res = vec![];
            res.extend(self.latest.clone());
            let previous_batches = try_join_all(v).await;
            for p in previous_batches? {
                res.extend(p);
            }
            Ok(res)
        }
        .boxed()
    }

    pub fn latest(&self) -> &VecDeque<(ChangesetId, Vec<ParentOffset>)> {
        &self.latest
    }

    pub fn previous_batches(&self) -> &VecDeque<FastlogBatchId> {
        &self.previous_batches
    }

    pub fn from_bytes(serialized: &Bytes) -> Result<FastlogBatch> {
        let thrift_entry: Result<thrift::FastlogBatch> = compact_protocol::deserialize(serialized)
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
            .with_context(|| ErrorKind::BlobDeserializeError("FastlogBatch".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::hash::Blake2;
    use borrowed::borrowed;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use memblob::Memblob;
    use pretty_assertions::assert_eq;
    use quickcheck::TestResult;
    use std::sync::Arc;

    #[fbinit::test]
    async fn test_fastlog_batch_empty(fb: FacebookInit) -> Result<()> {
        let blobstore = Arc::new(Memblob::default());
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx, blobstore: &Arc<_>);

        let list = VecDeque::new();
        let batch = FastlogBatch::new_from_raw_list(ctx, blobstore, list).await?;
        assert!(batch.latest.is_empty());
        assert!(batch.previous_batches.is_empty());

        Ok(())
    }

    #[fbinit::test]
    async fn test_fastlog_batch_single(fb: FacebookInit) -> Result<()> {
        let blobstore = Arc::new(Memblob::default());
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx, blobstore: &Arc<_>);

        let mut list = VecDeque::new();
        let csid = ChangesetId::new(Blake2::from_byte_array([1; 32]));
        list.push_back((csid, vec![]));
        let batch = FastlogBatch::new_from_raw_list(ctx, blobstore, list.clone()).await?;
        assert_eq!(batch.latest, list);
        assert!(batch.previous_batches.is_empty());

        Ok(())
    }

    #[fbinit::test]
    async fn test_fastlog_batch_large(fb: FacebookInit) -> Result<()> {
        let blobstore = Arc::new(Memblob::default());
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx, blobstore: &Arc<_>);

        let mut list = VecDeque::new();
        for i in 0..max_entries_in_fastlog_batch() {
            let csid = ChangesetId::new(Blake2::from_byte_array([i as u8; 32]));
            list.push_back((csid, vec![ParentOffset(1)]));
        }

        let batch = FastlogBatch::new_from_raw_list(ctx, blobstore, list.clone()).await?;
        assert_eq!(batch.latest.len(), MAX_LATEST_LEN);
        assert_eq!(batch.previous_batches.len(), MAX_BATCHES);

        let fetched_list = batch.fetch_raw_list(ctx, blobstore).await?;

        assert_eq!(fetched_list, Vec::from(list));
        Ok(())
    }

    #[fbinit::test]
    async fn test_fastlog_batch_overflow(fb: FacebookInit) -> Result<()> {
        let blobstore = Arc::new(Memblob::default());
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx, blobstore: &Arc<_>);

        let mut list = VecDeque::new();
        for i in 0..max_entries_in_fastlog_batch() + 1 {
            let csid = ChangesetId::new(Blake2::from_byte_array([i as u8; 32]));
            list.push_back((csid, vec![ParentOffset(1)]));
        }

        let batch = FastlogBatch::new_from_raw_list(ctx, blobstore, list.clone()).await?;
        assert_eq!(batch.latest.len(), MAX_LATEST_LEN);
        assert_eq!(batch.previous_batches.len(), MAX_BATCHES);

        let fetched_list = batch.fetch_raw_list(ctx, blobstore).await?;

        list.pop_back();
        assert_eq!(fetched_list, Vec::from(list));
        Ok(())
    }

    #[quickcheck_async::tokio]
    async fn fastlog_roundtrip(fb: FacebookInit, hashes: Vec<(ChangesetId, i32)>) -> TestResult {
        let blobstore = Arc::new(Memblob::default());
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx, blobstore: &Arc<_>);

        let mut raw_list = VecDeque::new();
        for (cs_id, offset) in hashes {
            raw_list.push_back((cs_id, vec![ParentOffset(offset)]));
        }

        let batch = FastlogBatch::new_from_raw_list(ctx, blobstore, raw_list.clone())
            .await
            .unwrap();

        if batch.latest.len() > MAX_LATEST_LEN {
            return TestResult::from_bool(false);
        }
        if batch.previous_batches.len() > MAX_BATCHES {
            return TestResult::from_bool(false);
        }

        raw_list.truncate(max_entries_in_fastlog_batch());
        let actual = batch.fetch_raw_list(ctx, blobstore).await.unwrap();

        TestResult::from_bool(raw_list == actual)
    }
}
