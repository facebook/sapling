/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::KeyedBlobstore;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;

use crate::FileType;
use crate::MPathElement;
use crate::ThriftConvert;
use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::blob::ContentManifestBlob;
use crate::sharded_map_v2::Rollup;
use crate::sharded_map_v2::ShardedMapV2Node;
use crate::sharded_map_v2::ShardedMapV2Value;
use crate::thrift;
use crate::typed_hash::ContentId;
use crate::typed_hash::ContentManifestId;
use crate::typed_hash::ContentManifestIdContext;
use crate::typed_hash::IdContext;
use crate::typed_hash::ShardedMapV2NodeContentManifestContext;
use crate::typed_hash::ShardedMapV2NodeContentManifestId;

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::content_manifest::ContentManifestFile)]
pub struct ContentManifestFile {
    pub content_id: ContentId,
    pub file_type: FileType,
    pub size: u64,
}

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::content_manifest::ContentManifestDirectory)]
pub struct ContentManifestDirectory {
    pub id: ContentManifestId,
    pub rollup_data: ContentManifestRollupData,
}

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::content_manifest::ContentManifestEntry)]
pub enum ContentManifestEntry {
    File(ContentManifestFile),
    Directory(ContentManifestDirectory),
}

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::content_manifest::ContentManifest)]
pub struct ContentManifest {
    pub subentries: ShardedMapV2Node<ContentManifestEntry>,
}

impl ContentManifest {
    pub fn empty() -> Self {
        Self {
            subentries: Default::default(),
        }
    }

    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl KeyedBlobstore,
        name: &MPathElement,
    ) -> Result<Option<ContentManifestEntry>> {
        self.subentries.lookup(ctx, blobstore, name.as_ref()).await
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl KeyedBlobstore,
    ) -> BoxStream<'a, Result<(MPathElement, ContentManifestEntry)>> {
        self.subentries
            .into_entries(ctx, blobstore)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_prefix_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl KeyedBlobstore,
        prefix: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, ContentManifestEntry)>> {
        self.subentries
            .into_prefix_entries(ctx, blobstore, prefix.as_ref())
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_prefix_subentries_after<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl KeyedBlobstore,
        prefix: &'a [u8],
        after: &'a [u8],
    ) -> BoxStream<'a, Result<(MPathElement, ContentManifestEntry)>> {
        self.subentries
            .into_prefix_entries_after(ctx, blobstore, prefix.as_ref(), after.as_ref())
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }

    pub fn into_subentries_skip<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl KeyedBlobstore,
        skip: usize,
    ) -> BoxStream<'a, Result<(MPathElement, ContentManifestEntry)>> {
        self.subentries
            .into_entries_skip(ctx, blobstore, skip)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }
}

impl BlobstoreValue for ContentManifest {
    type Key = ContentManifestId;

    fn into_blob(self) -> ContentManifestBlob {
        let data = self.into_bytes();
        let id = ContentManifestIdContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

impl ShardedMapV2Value for ContentManifestEntry {
    type NodeId = ShardedMapV2NodeContentManifestId;
    type Context = ShardedMapV2NodeContentManifestContext;
    type RollupData = ContentManifestRollupData;

    const WEIGHT_LIMIT: usize = 625;
}

#[derive(ThriftConvert, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[thrift(thrift::content_manifest::ContentManifestCounts)]
pub struct ContentManifestCounts {
    pub files_count: u64,
    pub dirs_count: u64,
    pub files_total_size: u64,
}

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash, Default)]
#[thrift(thrift::content_manifest::ContentManifestRollupData)]
pub struct ContentManifestRollupData {
    // Counts for the immediate children of this directory
    pub child_counts: ContentManifestCounts,
    // Counts for all descendants of this directory recursively
    pub descendant_counts: ContentManifestCounts,
}

impl Rollup<ContentManifestEntry> for ContentManifestRollupData {
    fn rollup(entry: Option<&ContentManifestEntry>, child_rollup_data: Vec<Self>) -> Self {
        child_rollup_data.into_iter().fold(
            ContentManifestRollupData {
                child_counts: entry.map_or(Default::default(), |entry| match entry {
                    ContentManifestEntry::File(f) => ContentManifestCounts {
                        files_count: 1,
                        dirs_count: 0,
                        files_total_size: f.size,
                    },
                    ContentManifestEntry::Directory(_) => ContentManifestCounts {
                        files_count: 0,
                        dirs_count: 1,
                        files_total_size: 0,
                    },
                }),
                descendant_counts: entry.map_or(Default::default(), |entry| match entry {
                    ContentManifestEntry::File(f) => ContentManifestCounts {
                        files_count: 1,
                        dirs_count: 0,
                        files_total_size: f.size,
                    },
                    ContentManifestEntry::Directory(d) => ContentManifestCounts {
                        files_count: d.rollup_data.descendant_counts.files_count,
                        dirs_count: d.rollup_data.descendant_counts.dirs_count + 1,
                        files_total_size: d.rollup_data.descendant_counts.files_total_size,
                    },
                }),
            },
            |acc, child| ContentManifestRollupData {
                child_counts: ContentManifestCounts {
                    files_count: acc.child_counts.files_count + child.child_counts.files_count,
                    dirs_count: acc.child_counts.dirs_count + child.child_counts.dirs_count,
                    files_total_size: acc.child_counts.files_total_size
                        + child.child_counts.files_total_size,
                },
                descendant_counts: ContentManifestCounts {
                    files_count: acc.descendant_counts.files_count
                        + child.descendant_counts.files_count,
                    dirs_count: acc.descendant_counts.dirs_count
                        + child.descendant_counts.dirs_count,
                    files_total_size: acc.descendant_counts.files_total_size
                        + child.descendant_counts.files_total_size,
                },
            },
        )
    }
}
