/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use edenapi_types::Blake3;
use edenapi_types::ContentId;
use edenapi_types::Sha1;
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

    pub(crate) fn content_id(data: &Bytes) -> ContentId {
        use blake2::digest::FixedOutput;
        use blake2::digest::Mac;
        use blake2::Blake2bMac;

        let mut hash =
            Blake2bMac::new_from_slice(b"content").expect("key to be less than 32 bytes");
        hash.update(data);
        let mut ret = [0; ContentId::len()];
        hash.finalize_into((&mut ret).into());
        ContentId::from_byte_array(ret)
    }

    pub(crate) fn sha1(data: &Bytes) -> Sha1 {
        use sha1::Digest;

        let mut hash = sha1::Sha1::new();
        hash.update(data);

        let bytes: [u8; Sha1::len()] = hash.finalize().into();
        Sha1::from(bytes)
    }

    pub(crate) fn seeded_blake3(data: &Bytes) -> Blake3 {
        #[cfg(not(fbcode_build))]
        {
            use blake3::Hasher;
            let key = "20220728-2357111317192329313741#".as_bytes();
            let mut ret = [0; Blake3::len()];
            ret.copy_from_slice(key);
            let mut hasher = Hasher::new_keyed(&ret);
            hasher.update(data.as_ref());
            let hashed_bytes: [u8; Blake3::len()] = hasher.finalize().into();
            Blake3::from(hashed_bytes)
        }
        #[cfg(fbcode_build)]
        {
            use blake3_c_ffi::Hasher;
            let key = blake3_constant::BLAKE3_HASH_KEY.as_bytes();
            let mut ret = [0; Blake3::len()];
            ret.copy_from_slice(key);
            let mut hasher = Hasher::new_keyed(&ret);
            hasher.update(data.as_ref());
            let mut hashed_bytes = [0; Blake3::len()];
            hasher.finalize(&mut hashed_bytes);
            Blake3::from(hashed_bytes)
        }
    }

    pub fn unwrap_sha256(self) -> Sha256 {
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
            StoreKey::HgId(ref key) => Some(key),
            StoreKey::Content(_, ref k) => k.as_ref(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_blake2() {
        #[rustfmt::skip]
        assert_eq!(
            ContentHash::content_id(&Bytes::from_static(b"abc")),
            ContentId::from([
                0x22, 0x8d, 0x7e, 0xfd, 0x5e, 0x3c, 0x1a, 0xcd,
                0xf4, 0x0e, 0x52, 0x43, 0x3f, 0x72, 0x8f, 0x53,
                0x78, 0x90, 0x0e, 0x41, 0xd4, 0xea, 0xe7, 0x14,
                0x64, 0x1f, 0x6f, 0x04, 0x0d, 0xee, 0x69, 0x3e,
            ])
        );
    }
}
