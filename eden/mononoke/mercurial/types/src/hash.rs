/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::str::FromStr;

use abomonation_derive::Abomonation;
use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use ascii::AsciiStr;
use ascii::AsciiString;
use faster_hex::hex_decode;
use faster_hex::hex_encode;
use quickcheck::single_shrinker;
use quickcheck::Arbitrary;
use quickcheck::Gen;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use sha1::Digest;

use crate::errors::ErrorKind;
use crate::thrift;

pub const SHA1_HASH_LENGTH_BYTES: usize = 20;
pub const SHA1_HASH_LENGTH_HEX: usize = SHA1_HASH_LENGTH_BYTES * 2;

pub const NULL: Sha1 = Sha1([0; SHA1_HASH_LENGTH_BYTES]);

/// Raw SHA-1 hash
///
/// Mercurial bases all its hashing on SHA-1, but this type is only used to build
/// more specific typed hashes.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(Serialize, Deserialize, Abomonation)]
pub struct Sha1([u8; SHA1_HASH_LENGTH_BYTES]);

impl Sha1 {
    /// Construct a `Sha1` from an array of SHA1_HASH_LENGTH_BYTES (20) bytes containing a
    /// SHA-1 (ie, *not* a hash of the bytes).
    pub fn from_bytes<B: AsRef<[u8]>>(bytes: B) -> Result<Sha1> {
        let bytes = bytes.as_ref();
        if bytes.len() != SHA1_HASH_LENGTH_BYTES {
            bail!(ErrorKind::InvalidSha1Input(format!(
                "need exactly {} bytes",
                SHA1_HASH_LENGTH_BYTES
            )));
        } else {
            let mut ret = [0; SHA1_HASH_LENGTH_BYTES];
            ret.copy_from_slice(bytes);
            Ok(Sha1(ret))
        }
    }

    pub fn from_thrift(h: thrift::Sha1) -> Result<Self> {
        if h.0.len() != SHA1_HASH_LENGTH_BYTES {
            bail!(ErrorKind::InvalidThrift(
                "Sha1".into(),
                format!(
                    "wrong length: expected {}, got {}",
                    SHA1_HASH_LENGTH_BYTES,
                    h.0.len()
                )
            ));
        }
        let mut arr = [0u8; SHA1_HASH_LENGTH_BYTES];
        arr.copy_from_slice(&h.0[..]);
        Ok(Sha1(arr))
    }

    /// Construct a `Sha1` from an array of SHA1_HASH_LENGTH_BYTES bytes.
    #[inline]
    pub const fn from_byte_array(arr: [u8; SHA1_HASH_LENGTH_BYTES]) -> Sha1 {
        Sha1(arr)
    }

    /// Extract this hash's underlying byte array.
    #[inline]
    pub(crate) fn into_byte_array(self) -> [u8; SHA1_HASH_LENGTH_BYTES] {
        self.0
    }

    /// Construct a `Sha1` from a hex-encoded `AsciiStr`.
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<Sha1> {
        Self::from_str(s.as_str())
    }

    pub fn to_hex(&self) -> AsciiString {
        let mut v = [0; SHA1_HASH_LENGTH_HEX];

        // This can only panic if buffer size of Vec isn't correct, which would be
        // a programming error.
        hex_encode(self.as_ref(), &mut v).expect("failed to hex encode");

        unsafe {
            // A hex string is always a pure ASCII string.
            AsciiString::from_ascii_unchecked(v)
        }
    }

    pub fn into_thrift(self) -> thrift::Sha1 {
        thrift::Sha1(self.0.into())
    }
}

/// Context for incrementally computing a `Sha1` hash.
#[derive(Clone)]
pub struct Context(sha1::Sha1);

/// Compute the `Sha1` for a slice of bytes.
impl<'a> From<&'a [u8]> for Sha1 {
    fn from(data: &[u8]) -> Sha1 {
        let mut sha1 = sha1::Sha1::new();
        sha1.update(data);

        Sha1(sha1.finalize().into())
    }
}

/// Get a reference to the underlying bytes of a `Sha1`
impl AsRef<[u8]> for Sha1 {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl FromStr for Sha1 {
    type Err = Error;

    fn from_str(s: &str) -> Result<Sha1> {
        if s.len() != SHA1_HASH_LENGTH_HEX {
            bail!(ErrorKind::InvalidSha1Input(format!(
                "need exactly {} hex digits",
                SHA1_HASH_LENGTH_HEX
            )));
        }

        let mut ret = Sha1([0; SHA1_HASH_LENGTH_BYTES]);
        match hex_decode(s.as_bytes(), &mut ret.0) {
            Ok(_) => Ok(ret),
            Err(_) => bail!(ErrorKind::InvalidSha1Input("bad hex character".into())),
        }
    }
}

impl Display for Sha1 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.to_hex(), fmt)
    }
}

/// Custom `Debug` output for `Sha1` so it prints in hex.
impl Debug for Sha1 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Sha1({})", self)
    }
}

impl Arbitrary for Sha1 {
    fn arbitrary(g: &mut Gen) -> Self {
        let mut bytes = [0; SHA1_HASH_LENGTH_BYTES];
        // The null hash is special, so give it a 5% chance of happening
        if usize::arbitrary(g) % SHA1_HASH_LENGTH_BYTES >= 1 {
            for b in bytes.iter_mut() {
                *b = u8::arbitrary(g);
            }
        }
        Sha1::from_bytes(&bytes).unwrap()
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        single_shrinker(NULL)
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(Serialize, Deserialize, Abomonation)]
/// Raw SHA-1 hash prefix.
/// Internal implementation is the inclusive range of Sha1 objects.
/// If can be build from a from a hex-encoded string (len <= SHA1_HASH_LENGTH_HEX (40))
/// or from an array of bytes (len <= SHA1_HASH_LENGTH_BYTES (20)).
pub struct Sha1Prefix(pub(crate) Sha1, pub(crate) Sha1);

impl Sha1Prefix {
    /// Construct a `Sha1Prefix` from an array of bytes.
    pub fn from_bytes<B: AsRef<[u8]> + ?Sized>(bytes: &B) -> Result<Self> {
        let bytes = bytes.as_ref();
        if bytes.len() > SHA1_HASH_LENGTH_BYTES {
            bail!(ErrorKind::InvalidSha1Input(format!(
                "prefix needs to be less or equal to {} bytes",
                SHA1_HASH_LENGTH_BYTES
            )))
        } else {
            static SHA1_MIN: [u8; SHA1_HASH_LENGTH_BYTES] = [0x00; SHA1_HASH_LENGTH_BYTES];
            static SHA1_MAX: [u8; SHA1_HASH_LENGTH_BYTES] = [0xff; SHA1_HASH_LENGTH_BYTES];

            let min_tail = &SHA1_MIN[bytes.len()..];
            let max_tail = &SHA1_MAX[bytes.len()..];

            Ok(Sha1Prefix(
                Sha1::from_bytes(&(bytes.iter().chain(min_tail).copied().collect::<Vec<_>>()))?,
                Sha1::from_bytes(&(bytes.iter().chain(max_tail).copied().collect::<Vec<_>>()))?,
            ))
        }
    }

    #[inline]
    /// Get a reference to the underlying bytes of the `Sha1` lower bound object.
    pub fn min_as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }

    #[inline]
    /// Get a reference to the underlying bytes of the `Sha1` inclusive upper bound object.
    pub fn max_as_ref(&self) -> &[u8] {
        self.1.as_ref()
    }

    /// Convert into sha1 if it is the full prefix of the hash.
    #[inline]
    pub fn into_sha1(self) -> Option<Sha1> {
        if self.0 == self.1 { Some(self.0) } else { None }
    }

    pub fn to_hex(&self) -> AsciiString {
        let mut v_min_hex = &mut [0; SHA1_HASH_LENGTH_HEX][..];
        hex_encode(self.0.as_ref(), v_min_hex).expect("failed to hex encode");
        let v_max_hex = &mut [0; SHA1_HASH_LENGTH_HEX][..];
        hex_encode(self.1.as_ref(), v_max_hex).expect("failed to hex encode");
        for i in 0..SHA1_HASH_LENGTH_HEX {
            if v_min_hex[i] != v_max_hex[i] {
                v_min_hex = &mut v_min_hex[..i];
                break;
            }
        }
        unsafe {
            // A hex string is always a pure ASCII string.
            AsciiString::from_ascii_unchecked(v_min_hex)
        }
    }
}

/// Construct a `Sha1Prefix` from a hex-encoded string.
impl FromStr for Sha1Prefix {
    type Err = Error;
    fn from_str(s: &str) -> Result<Sha1Prefix> {
        if s.len() > SHA1_HASH_LENGTH_HEX {
            bail!(ErrorKind::InvalidSha1Input(format!(
                "prefix needs to be less or equal {} hex digits",
                SHA1_HASH_LENGTH_HEX
            )));
        }
        let min_tail: String = String::from_utf8(vec![b'0'; SHA1_HASH_LENGTH_HEX - s.len()])?;
        let max_tail: String = String::from_utf8(vec![b'f'; SHA1_HASH_LENGTH_HEX - s.len()])?;
        Ok(Sha1Prefix(
            Sha1::from_str(&(s.to_owned() + &min_tail))?,
            Sha1::from_str(&(s.to_owned() + &max_tail))?,
        ))
    }
}

/// Custom `Display` output for `Sha1Prefix` so it prints in hex.
impl Display for Sha1Prefix {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.to_hex(), fmt)
    }
}

/// Custom `Debug` output for `Sha1Prefix` so it prints in hex.
impl Debug for Sha1Prefix {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Sha1Prefix({})", self)
    }
}

impl Context {
    /// Construct a `Context`
    pub fn new() -> Context {
        Context(sha1::Sha1::new())
    }

    /// Update a context from something that can be turned into a `&[u8]`
    pub fn update<T>(&mut self, data: T)
    where
        T: AsRef<[u8]>,
    {
        self.0.update(data.as_ref())
    }

    pub fn finish(self) -> Sha1 {
        Sha1(self.0.finalize().into())
    }
}

#[cfg(test)]
mod test {
    use quickcheck::quickcheck;
    use quickcheck::TestResult;
    use std::str::FromStr;

    use super::*;

    #[rustfmt::skip]
    const NILHASH: Sha1 = Sha1([0xda, 0x39, 0xa3, 0xee,
                                0x5e, 0x6b, 0x4b, 0x0d,
                                0x32, 0x55, 0xbf, 0xef,
                                0x95, 0x60, 0x18, 0x90,
                                0xaf, 0xd8, 0x07, 0x09]);

    #[test]
    fn test_null() {
        assert_eq!(NULL, Sha1([0_u8; SHA1_HASH_LENGTH_BYTES]));
    }

    #[test]
    fn test_nil() {
        let nil = Sha1::from(&[][..]);
        assert_eq!(nil, NILHASH);
    }

    #[test]
    fn parse_ok() {
        assert_eq!(
            NULL,
            Sha1::from_str("0000000000000000000000000000000000000000").unwrap()
        );
        assert_eq!(
            NILHASH,
            Sha1::from_str("da39a3ee5e6b4b0d3255bfef95601890afd80709").unwrap()
        );
        assert_eq!(
            NILHASH,
            Sha1::from_str("DA39A3EE5E6B4B0D3255BFEF95601890AFD80709").unwrap()
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(
            format!("{}", NULL),
            "0000000000000000000000000000000000000000"
        );
        assert_eq!(
            format!("{}", NILHASH),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn test_parse_and_display_prefix() {
        // even length
        assert_eq!(
            format!("{}", Sha1Prefix::from_str("da39a3").unwrap()),
            "da39a3"
        );
        // odd length
        assert_eq!(
            format!("{}", Sha1Prefix::from_str("da39a").unwrap()),
            "da39a"
        );
        // max length
        assert_eq!(
            format!(
                "{}",
                Sha1Prefix::from_str("da39a3ee5e6b4b0d3255bfef95601890afd80709").unwrap()
            ),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        // capital case
        assert_eq!(
            format!(
                "{}",
                Sha1Prefix::from_str("DA39A3EE5E6B4B0D3255BFEF95601890AFD80709").unwrap()
            ),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        // zero length
        assert_eq!(format!("{}", Sha1Prefix::from_str("").unwrap()), "");
    }

    #[test]
    fn parse_bad() {
        match Sha1::from_str("") {
            Ok(_) => panic!("unexpected OK - zero len"),
            Err(_) => {}
        };
        match Sha1::from_str("da39a3ee5e6b4b0d3255bfef95601890afd8070") {
            // one char missing
            Ok(_) => panic!("unexpected OK - trunc"),
            Err(_) => {}
        };
        match Sha1::from_str("xda39a3ee5e6b4b0d3255bfef95601890afd8070") {
            // one char bad
            Ok(_) => panic!("unexpected OK - badchar end"),
            Err(_) => {}
        };
        match Sha1::from_str("da39a3ee5e6b4b0d3255bfef95601890afd8070x") {
            // one char bad
            Ok(_) => panic!("unexpected OK - badchar end"),
            Err(_) => {}
        };
        match Sha1::from_str("da39a3ee5e6b4b0d325Xbfef95601890afd80709") {
            // one char missing
            Ok(_) => panic!("unexpected OK - trunc"),
            Err(_) => {}
        };
    }

    #[test]
    fn parse_thrift() {
        let null_thrift = thrift::Sha1(vec![0; SHA1_HASH_LENGTH_BYTES].into());
        assert_eq!(NULL, Sha1::from_thrift(null_thrift.clone()).unwrap());
        assert_eq!(NULL.into_thrift(), null_thrift);

        let nil_thrift = thrift::Sha1(NILHASH.0.into());
        assert_eq!(NILHASH, Sha1::from_thrift(nil_thrift.clone()).unwrap());
        assert_eq!(NILHASH.into_thrift(), nil_thrift);
    }

    #[test]
    fn parse_thrift_bad() {
        Sha1::from_thrift(thrift::Sha1(vec![].into())).expect_err("unexpected OK - zero len");
        Sha1::from_thrift(thrift::Sha1(vec![0; 19].into())).expect_err("unexpected OK - too short");
        Sha1::from_thrift(thrift::Sha1(vec![0; 21].into())).expect_err("unexpected OK - too long");
    }

    quickcheck! {
        fn parse_roundtrip(v: Vec<u8>) -> TestResult {
            if v.len() != SHA1_HASH_LENGTH_BYTES {
                return TestResult::discard()
            }
            let h = Sha1::from_bytes(v).unwrap();
            let s = format!("{}", h);
            let sh = s.parse().unwrap();

            TestResult::from_bool(h == sh)
        }

        fn thrift_roundtrip(h: Sha1) -> bool {
            let v = h.into_thrift();
            let sh = Sha1::from_thrift(v)
                .expect("converting a valid Thrift structure should always work");
            h == sh
        }

        fn to_hex_roundtrip(h: Sha1) -> bool {
            let v = h.to_hex();
            let sh = Sha1::from_ascii_str(&v).unwrap();
            h == sh
        }
    }
}
