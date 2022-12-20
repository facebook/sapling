/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use bytes::Bytes;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::StreamExt;

use crate::blob::BasenameSuffixSkeletonManifestBlob;
use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::sharded_map::MapValue;
use crate::sharded_map::ShardedMapNode;
use crate::thrift;
use crate::typed_hash::BasenameSuffixSkeletonManifestContext;
use crate::typed_hash::BasenameSuffixSkeletonManifestId;
use crate::typed_hash::IdContext;
use crate::typed_hash::ShardedMapNodeBSSMContext;
use crate::typed_hash::ShardedMapNodeBSSMId;
use crate::MPathElement;
use crate::ThriftConvert;

/// See docs/basename_suffix_skeleton_manifest.md for more documentation on this.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BasenameSuffixSkeletonManifest {
    subentries: ShardedMapNode<BssmEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BssmEntry {
    File,
    Directory(BssmDirectory),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BssmDirectory {
    pub id: BasenameSuffixSkeletonManifestId,
    pub rollup_count: u64,
}

impl BssmEntry {
    pub fn into_dir(self) -> Option<BssmDirectory> {
        match self {
            Self::File => None,
            Self::Directory(dir) => Some(dir),
        }
    }

    pub fn rollup_count(&self) -> u64 {
        match self {
            Self::File => 1,
            Self::Directory(dir) => dir.rollup_count,
        }
    }
}

#[async_trait]
impl Loadable for BssmDirectory {
    type Value = BasenameSuffixSkeletonManifest;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        self.id.load(ctx, blobstore).await
    }
}

impl ThriftConvert for BssmDirectory {
    const NAME: &'static str = "BssmDirectory";
    type Thrift = thrift::BssmDirectory;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        let thrift::BssmDirectory { id, rollup_count } = t;
        Ok(Self {
            id: ThriftConvert::from_thrift(id)?,
            rollup_count: rollup_count.try_into()?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::BssmDirectory {
            id: self.id.into_thrift(),
            rollup_count: self.rollup_count.try_into().unwrap_or(i64::MAX),
        }
    }
}

impl ThriftConvert for BssmEntry {
    const NAME: &'static str = "BssmEntry";
    type Thrift = thrift::BssmEntry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(match t {
            thrift::BssmEntry::file(thrift::BssmFile {}) => Self::File,
            thrift::BssmEntry::directory(dir) => Self::Directory(ThriftConvert::from_thrift(dir)?),
            thrift::BssmEntry::UnknownField(variant) => {
                anyhow::bail!("Unknown variant: {}", variant)
            }
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        match self {
            Self::File => thrift::BssmEntry::file(thrift::BssmFile {}),
            Self::Directory(dir) => thrift::BssmEntry::directory(dir.into_thrift()),
        }
    }
}

impl MapValue for BssmEntry {
    type Id = ShardedMapNodeBSSMId;
    type Context = ShardedMapNodeBSSMContext;
}

impl ThriftConvert for BasenameSuffixSkeletonManifest {
    const NAME: &'static str = "BasenameSuffixSkeletonManifest";
    type Thrift = thrift::BasenameSuffixSkeletonManifest;
    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            subentries: ShardedMapNode::from_thrift(t.subentries)?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::BasenameSuffixSkeletonManifest {
            subentries: self.subentries.into_thrift(),
        }
    }
}

type RollupCountDifference = i64;

impl BasenameSuffixSkeletonManifest {
    pub fn empty() -> Self {
        Self {
            subentries: ShardedMapNode::default(),
        }
    }
    pub async fn update(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        subentries_to_update: BTreeMap<MPathElement, Option<BssmEntry>>,
    ) -> Result<(Self, RollupCountDifference)> {
        let size_diff = AtomicI64::new(0);
        let subentries = self
            .subentries
            .update(
                ctx,
                blobstore,
                subentries_to_update
                    .into_iter()
                    .inspect(|(_, maybe_entry)| {
                        if let Some(new_entry) = maybe_entry {
                            size_diff.fetch_add(new_entry.rollup_count() as i64, Ordering::Relaxed);
                        }
                    })
                    .map(|(k, v)| (Bytes::copy_from_slice(k.as_ref()), v))
                    .collect(),
                |deleted| {
                    size_diff.fetch_sub(deleted.rollup_count() as i64, Ordering::Relaxed);
                },
            )
            .await?;
        Ok((Self { subentries }, size_diff.load(Ordering::Relaxed)))
    }

    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        name: &MPathElement,
    ) -> Result<Option<BssmEntry>> {
        self.subentries.lookup(ctx, blobstore, name.as_ref()).await
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPathElement, BssmEntry)>> {
        self.subentries
            .into_entries(ctx, blobstore)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }

    pub fn into_prefix_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, BssmEntry)>> {
        self.subentries
            .into_prefix_entries(ctx, blobstore, prefix)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }
}

impl BlobstoreValue for BasenameSuffixSkeletonManifest {
    type Key = BasenameSuffixSkeletonManifestId;

    fn into_blob(self) -> BasenameSuffixSkeletonManifestBlob {
        let data = self.into_bytes();
        let id = BasenameSuffixSkeletonManifestContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}
