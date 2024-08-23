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

use crate::blob::SkeletonManifestV2Blob;
use crate::sharded_map_v2::Rollup;
use crate::sharded_map_v2::ShardedMapV2Node;
use crate::sharded_map_v2::ShardedMapV2Value;
use crate::thrift;
use crate::typed_hash::IdContext;
use crate::typed_hash::ShardedMapV2NodeSkeletonManifestV2Context;
pub use crate::typed_hash::ShardedMapV2NodeSkeletonManifestV2Id;
use crate::typed_hash::SkeletonManifestV2Context;
use crate::typed_hash::SkeletonManifestV2Id;
use crate::Blob;
use crate::BlobstoreValue;
use crate::MPathElement;
use crate::ThriftConvert;

// See serialization/skeleton_manifest.thrift for more documentation.

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::skeleton_manifest::SkeletonManifestV2)]
pub struct SkeletonManifestV2 {
    pub subentries: ShardedMapV2Node<SkeletonManifestV2Entry>,
}

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::skeleton_manifest::SkeletonManifestV2Entry)]
pub enum SkeletonManifestV2Entry {
    #[thrift(thrift::skeleton_manifest::SkeletonManifestV2File)]
    File,
    Directory(SkeletonManifestV2),
}

impl SkeletonManifestV2Entry {
    pub fn into_dir(self) -> Option<SkeletonManifestV2> {
        match self {
            Self::File => None,
            Self::Directory(dir) => Some(dir),
        }
    }

    pub fn rollup_count(&self) -> SkeletonManifestV2RollupCount {
        match self {
            Self::File => SkeletonManifestV2RollupCount(1),
            Self::Directory(dir) => dir.rollup_count(),
        }
    }
}

#[async_trait]
impl Loadable for SkeletonManifestV2 {
    type Value = SkeletonManifestV2;

    async fn load<'a, B: Blobstore>(
        &'a self,
        _ctx: &'a CoreContext,
        _blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        Ok(self.clone())
    }
}

impl ShardedMapV2Value for SkeletonManifestV2Entry {
    type NodeId = ShardedMapV2NodeSkeletonManifestV2Id;
    type Context = ShardedMapV2NodeSkeletonManifestV2Context;
    type RollupData = SkeletonManifestV2RollupCount;

    const WEIGHT_LIMIT: usize = 500;

    // The weight function is overriden because the sharded map is stored
    // inlined in SkeletonManifestV2. So the weight of the sharded map
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
pub struct SkeletonManifestV2RollupCount(pub u64);

impl SkeletonManifestV2RollupCount {
    pub fn into_inner(self) -> u64 {
        self.0
    }
}

impl ThriftConvert for SkeletonManifestV2RollupCount {
    const NAME: &'static str = "SkeletonManifestV2RollupCount";
    type Thrift = i64;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(SkeletonManifestV2RollupCount(t as u64))
    }

    fn into_thrift(self) -> Self::Thrift {
        self.0 as i64
    }
}

impl Rollup<SkeletonManifestV2Entry> for SkeletonManifestV2RollupCount {
    fn rollup(value: Option<&SkeletonManifestV2Entry>, child_rollup_data: Vec<Self>) -> Self {
        child_rollup_data.into_iter().fold(
            value.map_or(SkeletonManifestV2RollupCount(0), |value| {
                value.rollup_count()
            }),
            |acc, child| SkeletonManifestV2RollupCount(acc.0 + child.0),
        )
    }
}

impl SkeletonManifestV2 {
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
    ) -> Result<Option<SkeletonManifestV2Entry>> {
        self.subentries.lookup(ctx, blobstore, name.as_ref()).await
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPathElement, SkeletonManifestV2Entry)>> {
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
    ) -> BoxStream<'a, Result<(MPathElement, SkeletonManifestV2Entry)>> {
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
    ) -> BoxStream<'a, Result<(MPathElement, SkeletonManifestV2Entry)>> {
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
    ) -> BoxStream<'a, Result<(MPathElement, SkeletonManifestV2Entry)>> {
        self.subentries
            .into_prefix_entries_after(ctx, blobstore, prefix, after)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }

    pub fn rollup_count(&self) -> SkeletonManifestV2RollupCount {
        SkeletonManifestV2RollupCount(1 + self.subentries.rollup_data().0)
    }
}

impl BlobstoreValue for SkeletonManifestV2 {
    type Key = SkeletonManifestV2Id;

    fn into_blob(self) -> SkeletonManifestV2Blob {
        let data = self.into_bytes();
        let id = SkeletonManifestV2Context::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}
