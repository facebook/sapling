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

use anyhow::{bail, Result};
use serde_derive::{Deserialize, Serialize};

/// A Sha256 hash.
#[derive(
    Clone,
    Copy,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
pub struct Sha256([u8; Sha256::len()]);

impl Sha256 {
    pub const fn len() -> usize {
        32
    }

    pub const fn hex_len() -> usize {
        Sha256::len() * 2
    }

    pub fn to_hex(&self) -> String {
        to_hex(self.0.as_ref())
    }

    pub fn from_hex(hex: &[u8]) -> Result<Self> {
        if hex.len() != Self::hex_len() {
            let msg = format!("{:?} is not a hex string of {} chars", hex, Self::hex_len());
            bail!(msg);
        }
        let mut bytes = [0u8; Self::len()];
        for (i, byte) in hex.iter().enumerate() {
            let value = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => {
                    let msg = format!("{:?} is not a hex character", *byte as char);
                    bail!(msg);
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

    pub fn from_byte_array(bytes: [u8; Self::len()]) -> Self {
        Self(bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Sha256::len() {
            bail!("invalid sha256 length {:?}", bytes.len());
        }

        let mut fixed_bytes = [0u8; Sha256::len()];
        fixed_bytes.copy_from_slice(bytes);
        Ok(Sha256(fixed_bytes))
    }

    pub fn into_inner(self) -> [u8; Sha256::len()] {
        self.0
    }
}

impl Display for Sha256 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.to_hex(), fmt)
    }
}

impl Debug for Sha256 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Sha256({:?})", &self.to_hex())
    }
}

impl<'a> From<&'a [u8; Sha256::len()]> for Sha256 {
    fn from(bytes: &[u8; Sha256::len()]) -> Sha256 {
        Sha256(bytes.clone())
    }
}

impl From<[u8; Sha256::len()]> for Sha256 {
    fn from(bytes: [u8; Sha256::len()]) -> Sha256 {
        Sha256(bytes)
    }
}

impl AsRef<[u8]> for Sha256 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl FromStr for Sha256 {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        if s.len() != Sha256::hex_len() {
            bail!("invalid sha256 length {:?}", s.len())
        }

        let mut ret = Sha256([0u8; Sha256::len()]);

        for idx in 0..ret.0.len() {
            ret.0[idx] = u8::from_str_radix(&s[(idx * 2)..(idx * 2 + 2)], 16)?;
        }

        Ok(ret)
    }
}

pub trait WriteSha256Ext {
    /// Write a `Sha256` to a stream.
    fn write_sha256(&mut self, value: &Sha256) -> io::Result<()>;
}

impl<W: Write + ?Sized> WriteSha256Ext for W {
    fn write_sha256(&mut self, value: &Sha256) -> io::Result<()> {
        self.write_all(&value.0)
    }
}

pub trait ReadSha256Ext {
    /// Read a `Sha256` from a stream.
    fn read_sha256(&mut self) -> io::Result<Sha256>;
}

impl<R: Read + ?Sized> ReadSha256Ext for R {
    fn read_sha256(&mut self) -> io::Result<Sha256> {
        let mut sha256 = Sha256([0u8; Sha256::len()]);
        self.read_exact(&mut sha256.0)?;
        Ok(sha256)
    }
}

const HEX_CHARS: &[u8] = b"0123456789abcdef";

pub fn to_hex(slice: &[u8]) -> String {
    let mut v = Vec::with_capacity(slice.len() * 2);
    for &byte in slice {
        v.push(HEX_CHARS[(byte >> 4) as usize]);
        v.push(HEX_CHARS[(byte & 0xf) as usize]);
    }

    unsafe { String::from_utf8_unchecked(v) }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for Sha256 {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let mut bytes = [0u8; Sha256::len()];
        g.fill_bytes(&mut bytes);
        Sha256::from(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;

    #[test]
    fn test_incorrect_length() {
        Sha256::from_slice(&[0u8; 25]).expect_err("bad slice length");
    }

    #[test]
    fn test_from_hex() {
        assert_eq!(
            Sha256::from_hex(b"abcd").unwrap_err().to_string(),
            "[97, 98, 99, 100] is not a hex string of 64 chars"
        );
        assert_eq!(
            Sha256::from_hex(&[b'z'; 64]).unwrap_err().to_string(),
            "\'z\' is not a hex character"
        );
        let hex = "434b1eeccd0fef2bad68f3c4f5dcbb2feb90b9465628a544cae3730ddf36310f";
        assert_eq!(Sha256::from_hex(hex.as_bytes()).unwrap().to_hex(), hex);
    }

    #[test]
    fn test_serde_with_using_cbor() {
        // Note: this test is for CBOR. Other serializers like mincode
        // or Thrift would have different backwards compatibility!
        use serde_cbor::de::from_slice as decode;
        use serde_cbor::ser::to_vec as encode;

        #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
        struct Orig(#[serde(with = "crate::serde_with::sha256::tuple")] Sha256);

        #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
        struct Bytes(#[serde(with = "crate::serde_with::sha256::bytes")] Sha256);

        #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
        struct Hex(#[serde(with = "crate::serde_with::sha256::hex")] Sha256);

        let id: Sha256 = Sha256([0xcc; Sha256::len()]);
        let orig = Orig(id);
        let bytes = Bytes(id);
        let hex = Hex(id);

        let cbor_orig = encode(&orig).unwrap();
        let cbor_bytes = encode(&bytes).unwrap();
        let cbor_hex = encode(&hex).unwrap();

        assert_eq!(cbor_orig.len(), 66);
        assert_eq!(cbor_bytes.len(), 34);
        assert_eq!(cbor_hex.len(), 66);

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
        fn test_from_slice(sha256: Sha256) -> bool {
            sha256 == Sha256::from_slice(sha256.as_ref()).expect("from_slice")
        }

        fn test_from_str(sha256: Sha256) -> bool {
            let hex = sha256.to_hex();
            sha256 == hex.parse().expect("FromStr")
        }
    }
}
