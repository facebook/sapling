/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::io::Read;
use std::io::Write;

use crate::hash::AbstractHashType;
use crate::hash::HashTypeInfo;

/// A Sha256 hash.
pub type Sha256 = AbstractHashType<Sha256TypeInfo, 32>;

pub struct Sha256TypeInfo;

impl HashTypeInfo for Sha256TypeInfo {
    const HASH_TYPE_NAME: &'static str = "Sha256";
}

impl Sha256 {
    pub fn into_inner(self) -> [u8; Self::len()] {
        self.into_byte_array()
    }
}

impl<'a> From<&'a [u8; Sha256::len()]> for Sha256 {
    fn from(bytes: &[u8; Sha256::len()]) -> Sha256 {
        Sha256::from_byte_array(bytes.clone())
    }
}

pub trait WriteSha256Ext {
    /// Write a `Sha256` to a stream.
    fn write_sha256(&mut self, value: &Sha256) -> io::Result<()>;
}

impl<W: Write + ?Sized> WriteSha256Ext for W {
    fn write_sha256(&mut self, value: &Sha256) -> io::Result<()> {
        self.write_all(value.as_ref())
    }
}

pub trait ReadSha256Ext {
    /// Read a `Sha256` from a stream.
    fn read_sha256(&mut self) -> io::Result<Sha256>;
}

impl<R: Read + ?Sized> ReadSha256Ext for R {
    fn read_sha256(&mut self) -> io::Result<Sha256> {
        let mut bytes = [0u8; Sha256::len()];
        self.read_exact(&mut bytes)?;
        let sha256 = Sha256::from_byte_array(bytes);
        Ok(sha256)
    }
}

// Some users dependent on it.
pub use crate::hash::to_hex;

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;
    use serde::Deserialize;
    use serde::Serialize;

    use super::*;

    #[test]
    fn test_incorrect_length() {
        Sha256::from_slice(&[0u8; 25]).expect_err("bad slice length");
    }

    #[test]
    fn test_from_hex() {
        assert_eq!(
            Sha256::from_hex(b"abcd").unwrap_err().to_string(),
            "[97, 98, 99, 100] is not a 64-byte hex string"
        );
        assert_eq!(
            Sha256::from_hex(&[b'z'; 64]).unwrap_err().to_string(),
            "[122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122, 122] is not a 64-byte hex string"
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

        let id: Sha256 = Sha256::from_byte_array([0xcc; Sha256::len()]);
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
