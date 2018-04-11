// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use ascii::{AsciiStr, AsciiString};
use bytes::Bytes;

use hash::Sha1;

use errors::*;

/// Representation of a blob of data.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[derive(Serialize, Deserialize)]
pub enum HgBlob {
    /// Modified data with no corresponding hash
    Dirty(Bytes),
    /// Clean data paired with its hash
    Clean(Bytes, HgBlobHash),
    /// External data; we only have its hash
    Extern(HgBlobHash),
}

/// Hash of a blob.
///
/// This is implemented as a `Sha1` hash.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
pub struct HgBlobHash(Sha1);

impl HgBlobHash {
    #[inline]
    pub fn new(sha1: Sha1) -> HgBlobHash {
        HgBlobHash(sha1)
    }

    /// Construct a `HgBlobHash` from an array of 20 bytes containing a SHA-1 (i.e. *not* a hash of
    /// the bytes).
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Result<HgBlobHash> {
        Sha1::from_bytes(bytes).map(HgBlobHash)
    }

    /// Construct a `HgBlobHash` from a hex-encoded `AsciiStr`.
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<HgBlobHash> {
        Sha1::from_ascii_str(s).map(HgBlobHash)
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

/// Compute the hash of a byte slice, resulting in a `HgBlobHash`
impl<'a> From<&'a [u8]> for HgBlobHash {
    fn from(data: &'a [u8]) -> Self {
        HgBlobHash(Sha1::from(data))
    }
}

impl HgBlob {
    /// Clean a `HgBlob` by computing its hash. Leaves non-`Dirty` blobs unchanged.
    pub fn clean(self) -> Self {
        match self {
            HgBlob::Dirty(data) => {
                let hash = HgBlobHash::from(data.as_ref());
                HgBlob::Clean(data, hash)
            }
            b @ HgBlob::Clean(..) | b @ HgBlob::Extern(..) => b,
        }
    }

    pub fn size(&self) -> Option<usize> {
        match self {
            &HgBlob::Dirty(ref data) => Some(data.len()),
            &HgBlob::Clean(ref data, _) => Some(data.as_ref().len()),
            &HgBlob::Extern(..) => None,
        }
    }

    pub fn as_inner(&self) -> Option<&Bytes> {
        match self {
            &HgBlob::Dirty(ref data) => Some(data),
            &HgBlob::Clean(ref data, _) => Some(data),
            &HgBlob::Extern(..) => None,
        }
    }

    pub fn hash(&self) -> Option<HgBlobHash> {
        match self {
            &HgBlob::Clean(_, hash) | &HgBlob::Extern(hash) => Some(hash),
            &HgBlob::Dirty(..) => None,
        }
    }

    pub fn into_inner(self) -> Option<Bytes> {
        match self {
            HgBlob::Dirty(data) => Some(data),
            HgBlob::Clean(data, _) => Some(data),
            HgBlob::Extern(..) => None,
        }
    }

    pub fn as_slice(&self) -> Option<&[u8]> {
        match self {
            &HgBlob::Dirty(ref data) => Some(data.as_ref()),
            &HgBlob::Clean(ref data, _) => Some(data.as_ref()),
            &HgBlob::Extern(..) => None,
        }
    }
}

impl From<Bytes> for HgBlob {
    fn from(data: Bytes) -> Self {
        HgBlob::Dirty(data)
    }
}

/// Get a reference to the `HgBlob`'s data, if it has some (ie, not `Extern`)
impl<'a> Into<Option<&'a [u8]>> for &'a HgBlob {
    fn into(self) -> Option<&'a [u8]> {
        match self {
            &HgBlob::Clean(ref data, _) => Some(data.as_ref()),
            &HgBlob::Dirty(ref data) => Some(data.as_ref()),
            &HgBlob::Extern(..) => None,
        }
    }
}

/// Construct an `Extern` `HgBlob` from a `HgBlobHash`
impl From<HgBlobHash> for HgBlob {
    fn from(bh: HgBlobHash) -> Self {
        HgBlob::Extern(bh)
    }
}

/// Get a reference for the underlying Sha1 bytes of a `HgBlobHash`
impl AsRef<[u8]> for HgBlobHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
