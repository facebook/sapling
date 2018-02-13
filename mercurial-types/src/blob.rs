// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use ascii::{AsciiStr, AsciiString};
use bytes::Bytes;

use super::NodeHash;
use hash::Sha1;

use errors::*;

/// Representation of a blob of data.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
pub enum Blob<T> {
    /// Modified data with no corresponding hash
    Dirty(T),
    /// Clean data paired with its hash
    Clean(T, BlobHash),
    /// External data; we only have its hash
    Extern(BlobHash),
    /// External data; we only have its nodeid
    NodeId(NodeHash),
}

/// Hash of a blob.
///
/// This is implemented as a `Sha1` hash.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
pub struct BlobHash(Sha1);

impl BlobHash {
    #[inline]
    pub fn new(sha1: Sha1) -> BlobHash {
        BlobHash(sha1)
    }

    /// Construct a `BlobHash` from an array of 20 bytes containing a SHA-1 (i.e. *not* a hash of
    /// the bytes).
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Result<BlobHash> {
        Sha1::from_bytes(bytes).map(BlobHash)
    }

    /// Construct a `BlobHash` from a hex-encoded `AsciiStr`.
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<BlobHash> {
        Sha1::from_ascii_str(s).map(BlobHash)
    }

    #[inline]
    pub fn sha1(&self) -> &Sha1 {
        &self.0
    }

    #[inline]
    pub fn to_hex(&self) -> AsciiString {
        self.0.to_hex()
    }
}

/// Compute the hash of a byte slice, resulting in a `BlobHash`
impl<'a> From<&'a [u8]> for BlobHash {
    fn from(data: &'a [u8]) -> Self {
        BlobHash(Sha1::from(data))
    }
}

impl<T> Blob<T>
where
    T: AsRef<[u8]>,
{
    /// Clean a `Blob` by computing its hash. Leaves non-`Dirty` blobs unchanged.
    pub fn clean(self) -> Self {
        match self {
            Blob::Dirty(data) => {
                let hash = BlobHash::from(data.as_ref());
                Blob::Clean(data, hash)
            }
            b @ Blob::Clean(..) | b @ Blob::Extern(..) | b @ Blob::NodeId(..) => b,
        }
    }

    pub fn size(&self) -> Option<usize> {
        match self {
            &Blob::Dirty(ref data) => Some(data.as_ref().len()),
            &Blob::Clean(_, ref data) => Some(data.as_ref().len()),
            &Blob::Extern(..) | &Blob::NodeId(..) => None,
        }
    }

    pub fn as_inner(&self) -> Option<&T> {
        match self {
            &Blob::Dirty(ref data) => Some(data),
            &Blob::Clean(ref data, _) => Some(data),
            &Blob::Extern(..) | &Blob::NodeId(..) => None,
        }
    }

    pub fn hash(&self) -> Option<BlobHash> {
        match self {
            &Blob::Clean(_, hash) | &Blob::Extern(hash) => Some(hash),
            &Blob::Dirty(..) | &Blob::NodeId(..) => None,
        }
    }

    pub fn into_inner(self) -> Option<T> {
        match self {
            Blob::Dirty(data) => Some(data),
            Blob::Clean(data, _) => Some(data),
            Blob::Extern(..) | Blob::NodeId(..) => None,
        }
    }

    pub fn as_slice(&self) -> Option<&[u8]> {
        match self {
            &Blob::Dirty(ref data) => Some(data.as_ref()),
            &Blob::Clean(ref data, _) => Some(data.as_ref()),
            &Blob::Extern(..) | &Blob::NodeId(..) => None,
        }
    }
}

/// Construct a dirty blob from raw data.
impl<'a> From<&'a [u8]> for Blob<Vec<u8>> {
    fn from(data: &'a [u8]) -> Self {
        Blob::Dirty(Vec::from(data))
    }
}

impl From<Vec<u8>> for Blob<Vec<u8>> {
    fn from(data: Vec<u8>) -> Self {
        Blob::Dirty(data)
    }
}

impl From<Bytes> for Blob<Bytes> {
    fn from(data: Bytes) -> Self {
        Blob::Dirty(data)
    }
}

/// Get a reference to the `Blob`'s data, if it has some (ie, not `Extern`)
impl<'a, T> Into<Option<&'a [u8]>> for &'a Blob<T>
where
    T: AsRef<[u8]>,
{
    fn into(self) -> Option<&'a [u8]> {
        match self {
            &Blob::Clean(ref data, _) => Some(data.as_ref()),
            &Blob::Dirty(ref data) => Some(data.as_ref()),
            &Blob::Extern(..) | &Blob::NodeId(..) => None,
        }
    }
}

/// Construct an `Extern` `Blob` from a `BlobHash`
impl<T> From<BlobHash> for Blob<T> {
    fn from(bh: BlobHash) -> Self {
        Blob::Extern(bh)
    }
}

/// Construct a `NodeId` `Blob` from a `NodeHash`
impl<T> From<NodeHash> for Blob<T> {
    fn from(id: NodeHash) -> Self {
        Blob::NodeId(id)
    }
}

/// Get a reference for the underlying Sha1 bytes of a `BlobHash`
impl AsRef<[u8]> for BlobHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
