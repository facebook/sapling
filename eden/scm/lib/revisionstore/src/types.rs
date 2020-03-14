/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bytes::Bytes;
use crypto::{digest::Digest, sha2::Sha256 as CryptoSha256};
use serde_derive::{Deserialize, Serialize};

#[cfg(any(test, feature = "for-tests"))]
use rand::Rng;

use types::{Key, Sha256};

/// Kind of content hash stored in the LFS pointer. Adding new types is acceptable, re-ordering or
/// removal is forbidden.
#[derive(
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Debug,
    Hash,
    Ord,
    PartialOrd,
    Clone
)]
pub enum ContentHash {
    Sha256(Sha256),
}

#[derive(Clone, PartialEq, Eq, Debug, Hash, Ord, PartialOrd)]
pub enum StoreKey {
    HgId(Key),
    Content(ContentHash),
}

impl ContentHash {
    pub fn sha256(data: &Bytes) -> Result<Self> {
        let mut hash = CryptoSha256::new();
        hash.input(data);

        let mut bytes = [0; Sha256::len()];
        hash.result(&mut bytes);
        Ok(ContentHash::Sha256(Sha256::from_slice(&bytes)?))
    }
}

impl StoreKey {
    pub fn hgid(key: Key) -> Self {
        StoreKey::HgId(key)
    }

    pub fn content(hash: ContentHash) -> Self {
        StoreKey::Content(hash)
    }
}

impl From<Key> for StoreKey {
    fn from(k: Key) -> Self {
        StoreKey::HgId(k)
    }
}

impl<'a> From<&'a Key> for StoreKey {
    fn from(k: &'a Key) -> Self {
        StoreKey::HgId(k.clone())
    }
}

impl From<ContentHash> for StoreKey {
    fn from(hash: ContentHash) -> Self {
        StoreKey::Content(hash)
    }
}

impl<'a> From<&'a ContentHash> for StoreKey {
    fn from(hash: &'a ContentHash) -> Self {
        StoreKey::Content(hash.clone())
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for ContentHash {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        ContentHash::Sha256(Sha256::arbitrary(g))
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for StoreKey {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        if g.gen() {
            StoreKey::HgId(Key::arbitrary(g))
        } else {
            StoreKey::Content(ContentHash::arbitrary(g))
        }
    }
}
