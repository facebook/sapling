// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Support for converting Mononoke data structures into in-memory blobs.

use blobstore::BlobstoreBytes;
use bytes::Bytes;

use crate::{
    errors::*,
    typed_hash::{
        ChangesetId, ContentChunkId, ContentId, ContentMetadataId, FastlogBatchId, FileUnodeId,
        FsnodeId, ManifestUnodeId, RawBundle2Id,
    },
};

/// A serialized blob in memory.
pub struct Blob<Id> {
    id: Id,
    data: Bytes,
}

impl<Id> Blob<Id> {
    pub fn new(id: Id, data: Bytes) -> Self {
        Self { id, data }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn data(&self) -> &Bytes {
        &self.data
    }
}

pub type ChangesetBlob = Blob<ChangesetId>;
pub type ContentBlob = Blob<ContentId>;
pub type ContentChunkBlob = Blob<ContentChunkId>;
pub type RawBundle2Blob = Blob<RawBundle2Id>;
pub type FileUnodeBlob = Blob<FileUnodeId>;
pub type ManifestUnodeBlob = Blob<ManifestUnodeId>;
pub type FsnodeBlob = Blob<FsnodeId>;
pub type ContentMetadataBlob = Blob<ContentMetadataId>;
pub type FastlogBatchBlob = Blob<FastlogBatchId>;

impl<Id> From<Blob<Id>> for BlobstoreBytes {
    #[inline]
    fn from(blob: Blob<Id>) -> BlobstoreBytes {
        BlobstoreBytes::from_bytes(blob.data)
    }
}

pub trait BlobstoreValue: Sized + Send {
    type Key;
    fn into_blob(self) -> Blob<Self::Key>;
    fn from_blob(blob: Blob<Self::Key>) -> Result<Self>;
}
