// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use crypto::{digest::Digest, sha1::Sha1, sha2::Sha256};
use std::convert::TryInto;

use mononoke_types::{hash, typed_hash, ContentId};

pub fn hash_bytes<H>(mut hasher: impl Hasher<H>, bytes: &Bytes) -> H
where
{
    hasher.update(&bytes);
    hasher.finish()
}

pub trait AdvisorySize {
    fn advise(&self) -> u64;
}

impl AdvisorySize for &Bytes {
    fn advise(&self) -> u64 {
        // NOTE: This will panic if the size of the Bytes buffer we have doesn't fit in a u64.
        self.len().try_into().unwrap()
    }
}

pub trait Hasher<H> {
    /// Update the Hasher with new bytes
    fn update<T: AsRef<[u8]>>(&mut self, bytes: T);

    /// Turn the Hasher into the actual Hash.
    fn finish(self) -> H;
}

pub struct ContentIdIncrementalHasher(typed_hash::ContentIdContext);

impl ContentIdIncrementalHasher {
    pub fn new() -> Self {
        Self(typed_hash::ContentIdContext::new())
    }
}

impl Hasher<ContentId> for ContentIdIncrementalHasher {
    fn update<T: AsRef<[u8]>>(&mut self, bytes: T) {
        self.0.update(bytes)
    }

    fn finish(self) -> ContentId {
        self.0.finish()
    }
}

pub struct Sha1IncrementalHasher(Sha1);

impl Sha1IncrementalHasher {
    pub fn new() -> Self {
        Self(Sha1::new())
    }
}

impl Hasher<hash::Sha1> for Sha1IncrementalHasher {
    fn update<T: AsRef<[u8]>>(&mut self, bytes: T) {
        self.0.input(bytes.as_ref())
    }

    fn finish(mut self) -> hash::Sha1 {
        let mut hash = [0u8; 20];
        self.0.result(&mut hash);
        hash::Sha1::from_byte_array(hash)
    }
}

pub struct Sha256IncrementalHasher(Sha256);

impl Sha256IncrementalHasher {
    pub fn new() -> Self {
        Self(Sha256::new())
    }
}

impl Hasher<hash::Sha256> for Sha256IncrementalHasher {
    fn update<T: AsRef<[u8]>>(&mut self, bytes: T) {
        self.0.input(bytes.as_ref())
    }

    fn finish(mut self) -> hash::Sha256 {
        let mut hash = [0u8; 32];
        self.0.result(&mut hash);
        hash::Sha256::from_byte_array(hash)
    }
}

pub struct GitSha1IncrementalHasher(Sha1, u64);

impl GitSha1IncrementalHasher {
    pub fn new<A: AdvisorySize>(size: A) -> Self {
        let size = size.advise();
        let mut sha1 = Sha1::new();
        let prototype = hash::GitSha1::from_byte_array([0; 20], "blob", size);
        sha1.input(&prototype.prefix());
        Self(sha1, size)
    }
}

impl Hasher<hash::GitSha1> for GitSha1IncrementalHasher {
    fn update<T: AsRef<[u8]>>(&mut self, bytes: T) {
        self.0.input(bytes.as_ref())
    }

    fn finish(mut self) -> hash::GitSha1 {
        let mut hash = [0u8; 20];
        self.0.result(&mut hash);
        hash::GitSha1::from_byte_array(hash, "blob", self.1)
    }
}
