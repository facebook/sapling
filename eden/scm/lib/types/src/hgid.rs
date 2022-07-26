/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use std::collections::HashSet;
use std::io;
use std::io::Read;
use std::io::Write;

#[cfg(any(test, feature = "for-tests"))]
use rand::RngCore;
use sha1::Digest;
use sha1::Sha1;

use crate::hash::AbstractHashType;
use crate::hash::HashTypeInfo;
use crate::parents::Parents;

/// A 20-byte identifier, often a hash. Nodes are used to uniquely identify
/// commits, file versions, and many other things.
///
///
/// # Serde Serialization
///
/// The `serde_with` module allows customization on `HgId` serialization:
/// - `#[serde(with = "types::serde_with::hgid::bytes")]` (current default)
/// - `#[serde(with = "types::serde_with::hgid::hex")]`
/// - `#[serde(with = "types::serde_with::hgid::tuple")]`
///
/// Using them can change the size or the type of serialization result:
///
/// | lib \ serde_with | hgid::tuple         | hgid::bytes  | hgid::hex |
/// |------------------|---------------------|--------------|-----------|
/// | mincode          | 20 bytes            | 21 bytes     | 41 bytes  |
/// | cbor             | 21 to 41 bytes  [1] | 21 bytes     | 42 bytes  |
/// | json             | 41 to 81+ bytes [1] | invalid  [2] | 42 bytes  |
/// | python           | Tuple[int]          | bytes        | str       |
///
/// In general,
/// - `hgid::tuple` only works best for `mincode`.
/// - `hgid::bytes` works best for cbor, python.
/// - `hgid::hex` is useful for `json`, or other text-only formats.
///
/// Compatibility note:
/// - `hgid::tuple` cannot decode `hgid::bytes` or `hgid::hex` data.
/// - `hgid::hex` can decode `hgid::bytes` data, or vice-versa. They share a
///   same `deserialize` implementation.
/// - `hgid::hex` or `hgid::bytes` might be able to decode `hgid::tuple` data,
///   depending on how tuples are serialized. For example, mincode
///   does not add framing for tuples, so `hgid::bytes` cannot decode
///   `hgid::tuple` data; cbor adds framing for tuples, so `hgid::bytes`
///   can decode `hgid::tuple` data.
///
/// [1]: Depends on actual data of `HgId`.
/// [2]: JSON only supports utf-8 data.
pub type HgId = AbstractHashType<HgIdTypeInfo, 20>;

pub struct HgIdTypeInfo;

impl HashTypeInfo for HgIdTypeInfo {
    const HASH_TYPE_NAME: &'static str = "HgId";
}

/// The nullid (0x00) is used throughout Mercurial to represent "None".
/// (For example, a commit will have a nullid p2, if it has no second parent).
pub const NULL_ID: HgId = HgId::from_byte_array([0; HgId::len()]);

/// The hard-coded 'working copy parent' Mercurial id.
pub const WDIR_ID: HgId = HgId::from_byte_array([0xff; HgId::len()]);

impl HgId {
    pub fn null_id() -> &'static Self {
        &NULL_ID
    }

    pub fn is_null(&self) -> bool {
        self == &NULL_ID
    }

    pub const fn wdir_id() -> &'static Self {
        &WDIR_ID
    }

    pub fn is_wdir(&self) -> bool {
        self == &WDIR_ID
    }

    pub fn from_content(data: &[u8], parents: Parents) -> Self {
        // Parents must be hashed in sorted order.
        let (p1, p2) = match parents.into_nodes() {
            (p1, p2) if p1 > p2 => (p2, p1),
            (p1, p2) => (p1, p2),
        };

        let mut hasher = Sha1::new();
        hasher.update(p1.as_ref());
        hasher.update(p2.as_ref());
        hasher.update(data);
        let hash: [u8; 20] = hasher.finalize().into();

        HgId::from_byte_array(hash)
    }

    #[cfg(any(test, feature = "for-tests"))]
    pub fn random(rng: &mut dyn RngCore) -> Self {
        let mut bytes = [0; HgId::len()];
        rng.fill_bytes(&mut bytes);
        loop {
            let hgid = HgId::from(&bytes);
            if !hgid.is_null() {
                return hgid;
            }
        }
    }

    #[cfg(any(test, feature = "for-tests"))]
    pub fn random_distinct(rng: &mut dyn RngCore, count: usize) -> Vec<Self> {
        let mut nodes = Vec::new();
        let mut nodeset = HashSet::new();
        while nodes.len() < count {
            let hgid = HgId::random(rng);
            if !nodeset.contains(&hgid) {
                nodeset.insert(hgid.clone());
                nodes.push(hgid);
            }
        }
        nodes
    }
}

impl<'a> From<&'a [u8; HgId::len()]> for HgId {
    fn from(bytes: &[u8; HgId::len()]) -> HgId {
        HgId::from_byte_array(bytes.clone())
    }
}

pub trait WriteHgIdExt {
    /// Write a ``HgId`` directly to a stream.
    ///
    /// # Examples
    ///
    /// ```
    /// use types::hgid::{HgId, WriteHgIdExt};
    /// let mut v = vec![];
    ///
    /// let n = HgId::null_id();
    /// v.write_hgid(&n).expect("writing a hgid to a vec should work");
    ///
    /// assert_eq!(v, vec![0; HgId::len()]);
    /// ```
    fn write_hgid(&mut self, value: &HgId) -> io::Result<()>;
}

impl<W: Write + ?Sized> WriteHgIdExt for W {
    fn write_hgid(&mut self, value: &HgId) -> io::Result<()> {
        self.write_all(value.as_ref())
    }
}

pub trait ReadHgIdExt {
    /// Read a ``HgId`` directly from a stream.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    /// use types::hgid::{HgId, ReadHgIdExt};
    /// let mut v = vec![0; HgId::len()];
    /// let mut c = Cursor::new(v);
    ///
    /// let n = c.read_hgid().expect("reading a hgid from a vec should work");
    ///
    /// assert_eq!(&n, HgId::null_id());
    /// ```
    fn read_hgid(&mut self) -> io::Result<HgId>;
}

impl<R: Read + ?Sized> ReadHgIdExt for R {
    fn read_hgid(&mut self) -> io::Result<HgId> {
        let mut bytes = [0; HgId::len()];
        self.read_exact(&mut bytes)?;
        Ok(HgId::from_byte_array(bytes))
    }
}

#[cfg(any(test, feature = "for-tests"))]
pub mod mocks {
    use super::HgId;

    pub const ONES: HgId = HgId::from_byte_array([0x11; HgId::len()]);
    pub const TWOS: HgId = HgId::from_byte_array([0x22; HgId::len()]);
    pub const THREES: HgId = HgId::from_byte_array([0x33; HgId::len()]);
    pub const FOURS: HgId = HgId::from_byte_array([0x44; HgId::len()]);
    pub const FIVES: HgId = HgId::from_byte_array([0x55; HgId::len()]);
    pub const SIXES: HgId = HgId::from_byte_array([0x66; HgId::len()]);
    pub const SEVENS: HgId = HgId::from_byte_array([0x77; HgId::len()]);
    pub const EIGHTS: HgId = HgId::from_byte_array([0x88; HgId::len()]);
    pub const NINES: HgId = HgId::from_byte_array([0x99; HgId::len()]);
    pub const AS: HgId = HgId::from_byte_array([0xAA; HgId::len()]);
    pub const BS: HgId = HgId::from_byte_array([0xAB; HgId::len()]);
    pub const CS: HgId = HgId::from_byte_array([0xCC; HgId::len()]);
    pub const DS: HgId = HgId::from_byte_array([0xDD; HgId::len()]);
    pub const ES: HgId = HgId::from_byte_array([0xEE; HgId::len()]);
    pub const FS: HgId = HgId::from_byte_array([0xFF; HgId::len()]);
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;
    use serde::Deserialize;
    use serde::Serialize;

    use super::*;

    #[test]
    fn test_incorrect_length() {
        HgId::from_slice(&[0u8; 25]).expect_err("bad slice length");
    }

    #[test]
    fn test_serde_with_using_cbor() {
        // Note: this test is for CBOR. Other serializers like mincode
        // or Thrift would have different backwards compatibility!
        use serde_cbor::de::from_slice as decode;
        use serde_cbor::ser::to_vec as encode;

        #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
        struct Orig(#[serde(with = "crate::serde_with::hgid::tuple")] HgId);

        #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
        struct Bytes(#[serde(with = "crate::serde_with::hgid::bytes")] HgId);

        #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
        struct Hex(#[serde(with = "crate::serde_with::hgid::hex")] HgId);

        let id: HgId = mocks::CS;
        let orig = Orig(id);
        let bytes = Bytes(id);
        let hex = Hex(id);

        let cbor_orig = encode(&orig).unwrap();
        let cbor_bytes = encode(&bytes).unwrap();
        let cbor_hex = encode(&hex).unwrap();

        assert_eq!(cbor_orig.len(), 41);
        assert_eq!(cbor_bytes.len(), 21);
        assert_eq!(cbor_hex.len(), 42);

        // Orig cannot decode bytes or hex.
        assert_eq!(decode::<Orig>(&cbor_orig).unwrap().0, id);
        decode::<Orig>(&cbor_bytes).unwrap_err();
        decode::<Orig>(&cbor_hex).unwrap_err();

        // Bytes can decode all 3 formats.
        assert_eq!(decode::<Bytes>(&cbor_orig).unwrap().0, id);
        assert_eq!(decode::<Bytes>(&cbor_bytes).unwrap().0, id);
        assert_eq!(decode::<Bytes>(&cbor_hex).unwrap().0, id);

        // Hex can decode all 3 formats.
        assert_eq!(decode::<Hex>(&cbor_orig).unwrap().0, id);
        assert_eq!(decode::<Hex>(&cbor_bytes).unwrap().0, id);
        assert_eq!(decode::<Hex>(&cbor_hex).unwrap().0, id);
    }

    quickcheck! {
        fn test_from_slice(hgid: HgId) -> bool {
            hgid == HgId::from_slice(hgid.as_ref()).expect("from_slice")
        }
    }
}
