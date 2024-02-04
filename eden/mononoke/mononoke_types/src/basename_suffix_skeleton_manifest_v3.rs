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

use crate::blob::BssmV3DirectoryBlob;
use crate::sharded_map_v2::Rollup;
use crate::sharded_map_v2::ShardedMapV2Node;
use crate::sharded_map_v2::ShardedMapV2Value;
use crate::thrift;
use crate::typed_hash::BssmV3DirectoryContext;
use crate::typed_hash::BssmV3DirectoryId;
use crate::typed_hash::IdContext;
use crate::typed_hash::ShardedMapV2NodeBssmV3Context;
pub use crate::typed_hash::ShardedMapV2NodeBssmV3Id;
use crate::Blob;
use crate::BlobstoreValue;
use crate::MPathElement;
use crate::ThriftConvert;

// See docs/basename_suffix_skeleton_manifest.md and mononoke_types_thrift.thrift
// for more documentation on this.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BssmV3Entry {
    File,
    Directory(BssmV3Directory),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BssmV3Directory {
    pub subentries: ShardedMapV2Node<BssmV3Entry>,
}

impl BssmV3Entry {
    pub fn into_dir(self) -> Option<BssmV3Directory> {
        match self {
            Self::File => None,
            Self::Directory(dir) => Some(dir),
        }
    }

    pub fn rollup_count(&self) -> BssmV3RollupCount {
        match self {
            Self::File => BssmV3RollupCount(1),
            Self::Directory(dir) => dir.rollup_count(),
        }
    }
}

#[async_trait]
impl Loadable for BssmV3Directory {
    type Value = BssmV3Directory;

    async fn load<'a, B: Blobstore>(
        &'a self,
        _ctx: &'a CoreContext,
        _blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        Ok(self.clone())
    }
}

impl ThriftConvert for BssmV3Directory {
    const NAME: &'static str = "BssmV3Directory";
    type Thrift = thrift::BssmV3Directory;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            subentries: ThriftConvert::from_thrift(t.subentries)?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::BssmV3Directory {
            subentries: self.subentries.into_thrift(),
        }
    }
}

impl ThriftConvert for BssmV3Entry {
    const NAME: &'static str = "BssmV3Entry";
    type Thrift = thrift::BssmV3Entry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(match t {
            thrift::BssmV3Entry::file(thrift::BssmV3File {}) => Self::File,
            thrift::BssmV3Entry::directory(dir) => {
                Self::Directory(ThriftConvert::from_thrift(dir)?)
            }
            thrift::BssmV3Entry::UnknownField(variant) => {
                anyhow::bail!("Unknown variant: {}", variant)
            }
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        match self {
            Self::File => thrift::BssmV3Entry::file(thrift::BssmV3File {}),
            Self::Directory(dir) => thrift::BssmV3Entry::directory(dir.into_thrift()),
        }
    }
}

impl ShardedMapV2Value for BssmV3Entry {
    type NodeId = ShardedMapV2NodeBssmV3Id;
    type Context = ShardedMapV2NodeBssmV3Context;
    type RollupData = BssmV3RollupCount;

    // The weight function is overrided because the sharded map is stored
    // inlined in BssmV3Directory. So the weight of the sharded map
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
pub struct BssmV3RollupCount(pub u64);

impl BssmV3RollupCount {
    pub fn into_inner(self) -> u64 {
        self.0
    }
}

impl ThriftConvert for BssmV3RollupCount {
    const NAME: &'static str = "BssmV3RollupCount";
    type Thrift = i64;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(BssmV3RollupCount(t as u64))
    }

    fn into_thrift(self) -> Self::Thrift {
        self.0 as i64
    }
}

impl Rollup<BssmV3Entry> for BssmV3RollupCount {
    fn rollup(value: Option<&BssmV3Entry>, child_rollup_data: Vec<Self>) -> Self {
        child_rollup_data.into_iter().fold(
            value.map_or(BssmV3RollupCount(0), |value| value.rollup_count()),
            |acc, child| BssmV3RollupCount(acc.0 + child.0),
        )
    }
}

impl BssmV3Directory {
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
    ) -> Result<Option<BssmV3Entry>> {
        self.subentries.lookup(ctx, blobstore, name.as_ref()).await
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPathElement, BssmV3Entry)>> {
        self.subentries
            .into_entries(ctx, blobstore)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_prefix_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, BssmV3Entry)>> {
        self.subentries
            .into_prefix_entries(ctx, blobstore, prefix)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }

    pub fn rollup_count(&self) -> BssmV3RollupCount {
        BssmV3RollupCount(1 + self.subentries.rollup_data().0)
    }
}

impl BlobstoreValue for BssmV3Directory {
    type Key = BssmV3DirectoryId;

    fn into_blob(self) -> BssmV3DirectoryBlob {
        let data = self.into_bytes();
        let id = BssmV3DirectoryContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}
