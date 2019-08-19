// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use serde_derive::{Deserialize, Serialize};

use blobstore::BlobstoreBytes;

// This used to have an Extern state earlier, which stood for the hash
// being present but the content not. This state ended up never being used in
// practice, but most of the methods still return Option types because of that.
// TODO(T28296583): Clean up HgBlob APIs to not return Option types

/// Representation of a blob of data.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[derive(Serialize, Deserialize)]
pub struct HgBlob(Bytes);

impl HgBlob {
    pub fn new(bytes: Bytes) -> Self {
        HgBlob(bytes)
    }

    pub fn size(&self) -> usize {
        self.0.len()
    }

    pub fn as_inner(&self) -> &Bytes {
        &self.0
    }

    pub fn into_inner(self) -> Bytes {
        self.0
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<Bytes> for HgBlob {
    fn from(data: Bytes) -> Self {
        HgBlob(data)
    }
}

impl From<Vec<u8>> for HgBlob {
    fn from(data: Vec<u8>) -> Self {
        HgBlob(data.into())
    }
}

/// Get a reference to the `HgBlob`'s data.
impl<'a> Into<&'a [u8]> for &'a HgBlob {
    fn into(self) -> &'a [u8] {
        self.0.as_ref()
    }
}

impl From<BlobstoreBytes> for HgBlob {
    #[inline]
    fn from(bytes: BlobstoreBytes) -> HgBlob {
        HgBlob(bytes.into_bytes())
    }
}

impl From<HgBlob> for BlobstoreBytes {
    #[inline]
    fn from(blob: HgBlob) -> BlobstoreBytes {
        BlobstoreBytes::from_bytes(blob.into_inner())
    }
}
