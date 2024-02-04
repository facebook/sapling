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
use futures::StreamExt;
use futures::TryStreamExt;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::blob::TestShardedManifestBlob;
use crate::sharded_map_v2::Rollup;
use crate::sharded_map_v2::ShardedMapV2Node;
use crate::sharded_map_v2::ShardedMapV2Value;
use crate::thrift;
use crate::typed_hash::IdContext;
use crate::typed_hash::ShardedMapV2NodeTestShardedManifestContext;
use crate::typed_hash::ShardedMapV2NodeTestShardedManifestId;
use crate::typed_hash::TestShardedManifestId;
use crate::typed_hash::TestShardedManifestIdContext;
use crate::MPathElement;
use crate::ThriftConvert;

/// A sharded version of TestManifest intended only to be used in tests.
/// It contains only the file names and the maximum basename length of all files
/// in each directory.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TestShardedManifest {
    pub subentries: ShardedMapV2Node<TestShardedManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TestShardedManifestEntry {
    File(TestShardedManifestFile),
    Directory(TestShardedManifestDirectory),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TestShardedManifestDirectory {
    pub id: TestShardedManifestId,
    pub max_basename_length: MaxBasenameLength,
}

impl ThriftConvert for TestShardedManifestDirectory {
    const NAME: &'static str = "TestShardedManifestDirectory";
    type Thrift = thrift::TestShardedManifestDirectory;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            id: ThriftConvert::from_thrift(t.id)?,
            max_basename_length: MaxBasenameLength(t.max_basename_length as u64),
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::TestShardedManifestDirectory {
            id: self.id.into_thrift(),
            max_basename_length: self.max_basename_length.0 as i64,
        }
    }
}

#[async_trait]
impl Loadable for TestShardedManifestDirectory {
    type Value = TestShardedManifest;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        self.id.load(ctx, blobstore).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TestShardedManifestFile {
    pub basename_length: u64,
}

impl ThriftConvert for TestShardedManifestFile {
    const NAME: &'static str = "TestShardedManifestFile";
    type Thrift = thrift::TestShardedManifestFile;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            basename_length: t.basename_length as u64,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::TestShardedManifestFile {
            basename_length: self.basename_length as i64,
        }
    }
}

impl ShardedMapV2Value for TestShardedManifestEntry {
    type NodeId = ShardedMapV2NodeTestShardedManifestId;
    type Context = ShardedMapV2NodeTestShardedManifestContext;
    type RollupData = MaxBasenameLength;
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct MaxBasenameLength(u64);

impl MaxBasenameLength {
    pub fn into_inner(self) -> u64 {
        self.0
    }
}

impl ThriftConvert for MaxBasenameLength {
    const NAME: &'static str = "MaxBasenameLength";
    type Thrift = i64;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self(t as u64))
    }

    fn into_thrift(self) -> Self::Thrift {
        self.0 as i64
    }
}

impl Rollup<TestShardedManifestEntry> for MaxBasenameLength {
    fn rollup(entry: Option<&TestShardedManifestEntry>, child_rollup_data: Vec<Self>) -> Self {
        child_rollup_data
            .into_iter()
            .chain(entry.map(|entry| match entry {
                TestShardedManifestEntry::Directory(dir) => dir.max_basename_length,
                TestShardedManifestEntry::File(file) => MaxBasenameLength(file.basename_length),
            }))
            .max()
            .unwrap_or_default()
    }
}

impl ThriftConvert for TestShardedManifestEntry {
    const NAME: &'static str = "TestShardedManifestEntry";
    type Thrift = thrift::TestShardedManifestEntry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(match t {
            thrift::TestShardedManifestEntry::file(file) => {
                Self::File(ThriftConvert::from_thrift(file)?)
            }
            thrift::TestShardedManifestEntry::directory(dir) => {
                Self::Directory(ThriftConvert::from_thrift(dir)?)
            }
            thrift::TestShardedManifestEntry::UnknownField(variant) => {
                anyhow::bail!("Unknown variant: {}", variant)
            }
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        match self {
            Self::File(file) => thrift::TestShardedManifestEntry::file(file.into_thrift()),
            Self::Directory(dir) => thrift::TestShardedManifestEntry::directory(dir.into_thrift()),
        }
    }
}

impl ThriftConvert for TestShardedManifest {
    const NAME: &'static str = "TestShardedManifest";
    type Thrift = thrift::TestShardedManifest;
    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            subentries: ShardedMapV2Node::from_thrift(t.subentries)?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        thrift::TestShardedManifest {
            subentries: self.subentries.into_thrift(),
        }
    }
}

impl BlobstoreValue for TestShardedManifest {
    type Key = TestShardedManifestId;

    fn into_blob(self) -> TestShardedManifestBlob {
        let data = self.into_bytes();
        let id = TestShardedManifestIdContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

impl TestShardedManifest {
    pub fn empty() -> Self {
        Self {
            subentries: Default::default(),
        }
    }

    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        name: &MPathElement,
    ) -> Result<Option<TestShardedManifestEntry>> {
        self.subentries.lookup(ctx, blobstore, name.as_ref()).await
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPathElement, TestShardedManifestEntry)>> {
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
    ) -> BoxStream<'a, Result<(MPathElement, TestShardedManifestEntry)>> {
        self.subentries
            .into_prefix_entries(ctx, blobstore, prefix)
            .map(|res| res.and_then(|(k, v)| anyhow::Ok((MPathElement::from_smallvec(k)?, v))))
            .boxed()
    }
}
