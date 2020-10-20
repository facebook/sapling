/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    fmt::{self, Debug, Display},
    io::{self, Read, Write},
    str::FromStr,
};

use anyhow::Result;
use serde::{de::Deserializer, Deserialize, Serialize};
use thiserror::Error;

use crate::parents::Parents;
use crate::sha::to_hex;
use sha1::{Digest, Sha1};

#[cfg(any(test, feature = "for-tests"))]
use rand::RngCore;

#[cfg(any(test, feature = "for-tests"))]
use std::collections::HashSet;

#[derive(Debug, Error)]
#[error("HgId Error: {0:?}")]
struct HgIdError(String);

/// A 20-byte identifier, often a hash. Nodes are used to uniquely identify
/// commits, file versions, and many other things.
///
///
/// # Serde Serialization
///
/// The `serde_with` module allows customization on `HgId` serialization:
/// - `#[serde(with = "types::serde_with::hgid::bytes")]`
/// - `#[serde(with = "types::serde_with::hgid::hex")]`
/// - `#[serde(with = "types::serde_with::hgid::tuple")]` (current default)
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
/// - `hgid::bytes` works best for cbor, python and probably should be the
///   default.
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
/// NOTE: Consider switching the default from `hgid::tuple` to `hgid::bytes`,
/// or dropping the default serialization implementation.
///
/// [1]: Depends on actual data of `HgId`.
/// [2]: JSON only supports utf-8 data.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct HgId([u8; HgId::len()]);

impl<'de> Deserialize<'de> for HgId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::serde_with::hgid::bytes::deserialize(deserializer)
    }
}

/// The nullid (0x00) is used throughout Mercurial to represent "None".
/// (For example, a commit will have a nullid p2, if it has no second parent).
pub const NULL_ID: HgId = HgId([0; HgId::len()]);

/// The hard-coded 'working copy parent' Mercurial id.
pub const WDIR_ID: HgId = HgId([0xff; HgId::len()]);

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

    pub const fn len() -> usize {
        20
    }

    pub const fn hex_len() -> usize {
        40
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != HgId::len() {
            return Err(HgIdError(format!("invalid hgid length {:?}", bytes.len())).into());
        }

        let mut fixed_bytes = [0u8; HgId::len()];
        fixed_bytes.copy_from_slice(bytes);
        Ok(HgId(fixed_bytes))
    }

    pub fn from_content(data: &[u8], parents: Parents) -> Self {
        // Parents must be hashed in sorted order.
        let (p1, p2) = match parents.into_nodes() {
            (p1, p2) if p1 > p2 => (p2, p1),
            (p1, p2) => (p1, p2),
        };

        let mut hasher = Sha1::new();
        hasher.input(p1.as_ref());
        hasher.input(p2.as_ref());
        hasher.input(data);
        let hash: [u8; 20] = hasher.result().into();

        HgId::from_byte_array(hash)
    }

    pub fn from_byte_array(bytes: [u8; HgId::len()]) -> Self {
        HgId(bytes)
    }

    pub fn into_byte_array(self) -> [u8; HgId::len()] {
        self.0
    }

    pub fn to_hex(&self) -> String {
        to_hex(self.0.as_ref())
    }

    pub fn from_hex(hex: &[u8]) -> Result<Self> {
        if hex.len() != Self::hex_len() {
            let msg = format!("{:?} is not a hex string of {} chars", hex, Self::hex_len());
            return Err(HgIdError(msg).into());
        }
        let mut bytes = [0u8; Self::len()];
        for (i, byte) in hex.iter().enumerate() {
            let value = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => {
                    let msg = format!("{:?} is not a hex character", *byte as char);
                    return Err(HgIdError(msg).into());
                }
            };
            if i & 1 == 0 {
                bytes[i / 2] |= value << 4;
            } else {
                bytes[i / 2] |= value;
            }
        }
        Ok(Self::from_byte_array(bytes))
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

impl Display for HgId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.to_hex(), fmt)
    }
}

impl Debug for HgId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "HgId({:?})", &self.to_hex())
    }
}

impl Default for HgId {
    fn default() -> HgId {
        NULL_ID
    }
}

impl<'a> From<&'a [u8; HgId::len()]> for HgId {
    fn from(bytes: &[u8; HgId::len()]) -> HgId {
        HgId(bytes.clone())
    }
}

impl AsRef<[u8]> for HgId {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl FromStr for HgId {
    type Err = anyhow::Error;

    // Taken from Mononoke
    fn from_str(s: &str) -> Result<Self> {
        if s.len() != HgId::hex_len() {
            return Err(HgIdError(format!("invalid string length {:?}", s.len())).into());
        }

        let mut ret = HgId([0u8; HgId::len()]);

        for idx in 0..ret.0.len() {
            ret.0[idx] = match u8::from_str_radix(&s[(idx * 2)..(idx * 2 + 2)], 16) {
                Ok(v) => v,
                Err(_) => return Err(HgIdError("bad digit".to_string()).into()),
            }
        }

        Ok(ret)
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
        self.write_all(&value.0)
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
        let mut hgid = HgId([0u8; HgId::len()]);
        self.read_exact(&mut hgid.0)?;
        Ok(hgid)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for HgId {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let mut bytes = [0u8; HgId::len()];
        g.fill_bytes(&mut bytes);
        HgId::from(&bytes)
    }
}

#[cfg(any(test, feature = "for-tests"))]
pub mod mocks {
    use super::HgId;

    pub const ONES: HgId = HgId([0x11; HgId::len()]);
    pub const TWOS: HgId = HgId([0x22; HgId::len()]);
    pub const THREES: HgId = HgId([0x33; HgId::len()]);
    pub const FOURS: HgId = HgId([0x44; HgId::len()]);
    pub const FIVES: HgId = HgId([0x55; HgId::len()]);
    pub const SIXES: HgId = HgId([0x66; HgId::len()]);
    pub const SEVENS: HgId = HgId([0x77; HgId::len()]);
    pub const EIGHTS: HgId = HgId([0x88; HgId::len()]);
    pub const NINES: HgId = HgId([0x99; HgId::len()]);
    pub const AS: HgId = HgId([0xAA; HgId::len()]);
    pub const BS: HgId = HgId([0xAB; HgId::len()]);
    pub const CS: HgId = HgId([0xCC; HgId::len()]);
    pub const DS: HgId = HgId([0xDD; HgId::len()]);
    pub const ES: HgId = HgId([0xEE; HgId::len()]);
    pub const FS: HgId = HgId([0xFF; HgId::len()]);
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;

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
