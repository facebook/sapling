/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::AsRef;

use blake3::Hasher as Blake3;
use bytes::Bytes;
use digest::Digest;
use mononoke_types::hash;
use mononoke_types::typed_hash;
use mononoke_types::ContentId;
use sha1::Sha1;
use sha2::Sha256;

pub fn hash_bytes<H>(mut hasher: impl Hasher<H>, bytes: impl AsRef<[u8]>) -> H {
    hasher.update(bytes);
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
        self.0.update(bytes.as_ref())
    }

    fn finish(self) -> hash::Sha1 {
        let hash = self.0.finalize().into();
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
        self.0.update(bytes.as_ref())
    }

    fn finish(self) -> hash::Sha256 {
        let hash = self.0.finalize().into();
        hash::Sha256::from_byte_array(hash)
    }
}

pub struct GitSha1IncrementalHasher(Sha1, u64);

impl GitSha1IncrementalHasher {
    pub fn new<A: AdvisorySize>(size: A) -> Self {
        let size = size.advise();
        let mut sha1 = Sha1::new();
        let prototype = hash::RichGitSha1::from_byte_array([0; 20], "blob", size);
        sha1.update(&prototype.prefix());
        Self(sha1, size)
    }
}

impl Hasher<hash::RichGitSha1> for GitSha1IncrementalHasher {
    fn update<T: AsRef<[u8]>>(&mut self, bytes: T) {
        self.0.update(bytes.as_ref())
    }

    fn finish(self) -> hash::RichGitSha1 {
        let hash = self.0.finalize().into();
        hash::RichGitSha1::from_byte_array(hash, "blob", self.1)
    }
}

/// Incremental hasher for non-seeded and seeded Blake3 hash
pub struct Blake3IncrementalHasher(Blake3);

impl Blake3IncrementalHasher {
    /// Creates a non-seeded Blake3 incremental hasher
    pub fn _new() -> Self {
        Self(Blake3::new())
    }

    /// Creates a seeded Blake3 incremental hasher
    pub fn new_seeded() -> Self {
        #[cfg(fbcode_build)]
        let bytes = blake3_constant::BLAKE3_HASH_KEY.as_bytes();
        #[cfg(not(fbcode_build))]
        let bytes = "20220728-2357111317192329313741#".as_bytes();
        if bytes.len() != blake3::KEY_LEN {
            panic!(
                "Seed for Blake3 hash needs exactly {} bytes",
                blake3::KEY_LEN
            )
        } else {
            let mut ret = [0; blake3::KEY_LEN];
            ret.copy_from_slice(bytes);
            Self(Blake3::new_keyed(&ret))
        }
    }
}

impl Hasher<hash::Blake3> for Blake3IncrementalHasher {
    fn update<T: AsRef<[u8]>>(&mut self, bytes: T) {
        self.0.update(bytes.as_ref());
    }

    fn finish(self) -> hash::Blake3 {
        let hash = self.0.finalize().into();
        hash::Blake3::from_byte_array(hash)
    }
}
