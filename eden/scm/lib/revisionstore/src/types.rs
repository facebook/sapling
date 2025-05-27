/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use minibytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use types::Key;
use types::Sha256;

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
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum ContentHash {
    Sha256(#[serde(with = "types::serde_with::sha256::tuple")] Sha256),
}

#[derive(Clone, PartialEq, Eq, Debug, Hash, Ord, PartialOrd)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum StoreKey {
    HgId(Key),
    /// The Key is a temporary workaround to being able to fallback from the LFS protocol to the
    /// non-LFS protocol. Do not depend on it as it will be removed.
    Content(ContentHash, Option<Key>),
}

impl ContentHash {
    pub fn sha256(data: &Bytes) -> Self {
        use sha2::Digest;

        let mut hash = sha2::Sha256::new();
        hash.update(data);

        let bytes: [u8; Sha256::len()] = hash.finalize().into();
        ContentHash::Sha256(Sha256::from(bytes))
    }

    pub fn unwrap_sha256(self) -> Sha256 {
        match self {
            ContentHash::Sha256(hash) => hash,
        }
    }

    pub(crate) fn sha256_ref(&self) -> &Sha256 {
        match self {
            ContentHash::Sha256(hash) => hash,
        }
    }
}

impl StoreKey {
    pub fn hgid(key: Key) -> Self {
        StoreKey::HgId(key)
    }

    pub fn content(hash: ContentHash) -> Self {
        StoreKey::Content(hash, None)
    }

    pub fn maybe_into_key(self) -> Option<Key> {
        match self {
            StoreKey::HgId(key) => Some(key),
            StoreKey::Content(_, k) => k,
        }
    }

    pub fn maybe_as_key(&self) -> Option<&Key> {
        match self {
            StoreKey::HgId(key) => Some(key),
            StoreKey::Content(_, k) => k.as_ref(),
        }
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
        StoreKey::Content(hash, None)
    }
}

impl<'a> From<&'a ContentHash> for StoreKey {
    fn from(hash: &'a ContentHash) -> Self {
        StoreKey::Content(hash.clone(), None)
    }
}
