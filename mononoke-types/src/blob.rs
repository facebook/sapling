// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Support for converting Mononoke data structures into in-memory blobs.

use bytes::Bytes;

use typed_hash::{ChangesetId, ContentId};

/// A serialized blob in memory.
pub struct Blob<Id> {
    id: Id,
    data: Bytes,
}

impl<Id> Blob<Id> {
    pub(crate) fn new(id: Id, data: Bytes) -> Self {
        Self { id, data }
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
