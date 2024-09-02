/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;

use crate::blob::CaseConflictSkeletonManifestBlob;
use crate::sharded_map_v2::Rollup;
use crate::sharded_map_v2::ShardedMapV2Node;
use crate::sharded_map_v2::ShardedMapV2Value;
use crate::thrift;
use crate::typed_hash::CaseConflictSkeletonManifestContext;
use crate::typed_hash::CaseConflictSkeletonManifestId;
use crate::typed_hash::IdContext;
use crate::typed_hash::ShardedMapV2NodeCcsmContext;
pub use crate::typed_hash::ShardedMapV2NodeCcsmId;
use crate::Blob;
use crate::BlobstoreValue;
use crate::MPathElement;
use crate::ThriftConvert;

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::ccsm::CcsmEntry)]
pub enum CcsmEntry {
    #[thrift(thrift::ccsm::CcsmFile)]
    File,
    Directory(CaseConflictSkeletonManifest),
}

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::ccsm::CaseConflictSkeletonManifest)]
pub struct CaseConflictSkeletonManifest {
    pub subentries: ShardedMapV2Node<CcsmEntry>,
}

impl CcsmEntry {
    pub fn into_dir(self) -> Option<CaseConflictSkeletonManifest> {
        match self {
            Self::File => None,
            Self::Directory(dir) => Some(dir),
        }
    }

    pub fn rollup_count(&self) -> CcsmRollupCount {
        match self {
            Self::File => CcsmRollupCount(1),
            Self::Directory(dir) => dir.rollup_count(),
        }
    }
}

#[async_trait]
impl Loadable for CaseConflictSkeletonManifest {
    type Value = CaseConflictSkeletonManifest;

    async fn load<'a, B: Blobstore>(
        &'a self,
        _ctx: &'a CoreContext,
        _blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        Ok(self.clone())
    }
}

impl ShardedMapV2Value for CcsmEntry {
    type NodeId = ShardedMapV2NodeCcsmId;
    type Context = ShardedMapV2NodeCcsmContext;
    type RollupData = CcsmRollupCount;

    const WEIGHT_LIMIT: usize = 1000;

    // The weight function is overridden because the sharded map is stored
    // inlined in CaseConflictSkeletonManifest. So the weight of the sharded map
    // should be propagated to make sure each sharded map blob stays
    // within the weight limit.
    fn weight(&self) -> usize {
        match self {
            Self::File => 1,
            // This `1 +` is needed to offset the extra space required for
            // the bytes that represent the path element to this directory.
            Self::Directory(dir) => 1 + dir.subentries.weight(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CcsmRollupCount(pub u64);

impl CcsmRollupCount {
    pub fn into_inner(self) -> u64 {
        self.0
    }
}

impl ThriftConvert for CcsmRollupCount {
    const NAME: &'static str = "CcsmRollupCount";
    type Thrift = i64;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(CcsmRollupCount(t as u64))
    }

    fn into_thrift(self) -> Self::Thrift {
        self.0 as i64
    }
}

impl Rollup<CcsmEntry> for CcsmRollupCount {
    fn rollup(value: Option<&CcsmEntry>, child_rollup_data: Vec<Self>) -> Self {
        child_rollup_data.into_iter().fold(
            value.map_or(CcsmRollupCount(0), |value| value.rollup_count()),
            |acc, child| CcsmRollupCount(acc.0 + child.0),
        )
    }
}

impl CaseConflictSkeletonManifest {
    pub fn empty() -> Self {
        Self {
            subentries: ShardedMapV2Node::default(),
        }
    }

    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        name: &MPathElement,
    ) -> Result<Option<CcsmEntry>> {
        self.subentries.lookup(ctx, blobstore, name.as_ref()).await
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPathElement, CcsmEntry)>> {
        self.subentries
            .into_entries(ctx, blobstore)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_subentries_skip<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        skip: usize,
    ) -> BoxStream<'a, Result<(MPathElement, CcsmEntry)>> {
        self.subentries
            .into_entries_skip(ctx, blobstore, skip)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_prefix_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, CcsmEntry)>> {
        self.subentries
            .into_prefix_entries(ctx, blobstore, prefix)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }

    pub fn into_prefix_subentries_after<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
        after: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, CcsmEntry)>> {
        self.subentries
            .into_prefix_entries_after(ctx, blobstore, prefix, after)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }

    pub fn rollup_count(&self) -> CcsmRollupCount {
        CcsmRollupCount(1 + self.subentries.rollup_data().0)
    }
}

impl BlobstoreValue for CaseConflictSkeletonManifest {
    type Key = CaseConflictSkeletonManifestId;

    fn into_blob(self) -> CaseConflictSkeletonManifestBlob {
        let data = self.into_bytes();
        let id = CaseConflictSkeletonManifestContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}
