// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use crypto::{digest::Digest, sha1::Sha1, sha2::Sha256};

use mononoke_types::{hash, typed_hash, ContentId};

pub fn hash_bytes<H>(bytes: &Bytes) -> H
where
    H: Hashable,
{
    let len = bytes.len() as u64;
    let mut hasher = H::Hasher::new(len);
    hasher.update(&bytes);
    hasher.finish()
}

pub trait Hasher<H> {
    /// Create a new Hasher
    fn new(size: u64) -> Self;

    /// Update the Hasher with new bytes
    fn update<T: AsRef<[u8]>>(&mut self, bytes: T);

    /// Turn the Hasher into the actual Hash.
    fn finish(self) -> H;
}

pub struct ContentIdIncrementalHasher(typed_hash::ContentIdContext);

impl Hasher<ContentId> for ContentIdIncrementalHasher {
    fn new(_size: u64) -> Self {
        Self(typed_hash::ContentIdContext::new())
    }

    fn update<T: AsRef<[u8]>>(&mut self, bytes: T) {
        self.0.update(bytes)
    }

    fn finish(self) -> ContentId {
        self.0.finish()
    }
}

pub struct Sha1IncrementalHasher(Sha1);

impl Hasher<hash::Sha1> for Sha1IncrementalHasher {
    fn new(_size: u64) -> Self {
        Self(Sha1::new())
    }

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

impl Hasher<hash::Sha256> for Sha256IncrementalHasher {
    fn new(_size: u64) -> Self {
        Self(Sha256::new())
    }

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

impl Hasher<hash::GitSha1> for GitSha1IncrementalHasher {
    fn new(size: u64) -> Self {
        let mut sha1 = Sha1::new();
        let prototype = hash::GitSha1::from_byte_array([0; 20], "blob", size);
        sha1.input(&prototype.prefix());
        Self(sha1, size)
    }

    fn update<T: AsRef<[u8]>>(&mut self, bytes: T) {
        self.0.input(bytes.as_ref())
    }

    fn finish(mut self) -> hash::GitSha1 {
        let mut hash = [0u8; 20];
        self.0.result(&mut hash);
        hash::GitSha1::from_byte_array(hash, "blob", self.1)
    }
}

pub trait Hashable: Sized {
    type Hasher: Hasher<Self>;
}

impl Hashable for ContentId {
    type Hasher = ContentIdIncrementalHasher;
}

impl Hashable for hash::Sha1 {
    type Hasher = Sha1IncrementalHasher;
}

impl Hashable for hash::Sha256 {
    type Hasher = Sha256IncrementalHasher;
}

impl Hashable for hash::GitSha1 {
    type Hasher = GitSha1IncrementalHasher;
}
