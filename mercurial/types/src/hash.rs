/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt::{self, Debug, Display};
use std::str::FromStr;

use abomonation_derive::Abomonation;
use ascii::{AsciiStr, AsciiString};
use crypto::{digest::Digest, sha1};
use failure_ext::bail;
use faster_hex::{hex_decode, hex_encode};
use heapsize_derive::HeapSizeOf;
use quickcheck::{single_shrinker, Arbitrary, Gen};
use rand::Rng;
use serde_derive::{Deserialize, Serialize};

use crate::errors::*;
use crate::thrift;

pub const NULL: Sha1 = Sha1([0; 20]);

/// Raw SHA-1 hash
///
/// Mercurial bases all its hashing on SHA-1, but this type is only used to build
/// more specific typed hashes.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(Serialize, Deserialize, HeapSizeOf, Abomonation)]
pub struct Sha1([u8; 20]);

impl Sha1 {
    /// Construct a `Sha1` from an array of 20 bytes containing a
    /// SHA-1 (ie, *not* a hash of the bytes).
    pub fn from_bytes<B: AsRef<[u8]>>(bytes: B) -> Result<Sha1> {
        let bytes = bytes.as_ref();
        if bytes.len() != 20 {
            bail!(ErrorKind::InvalidSha1Input("need exactly 20 bytes".into()));
        } else {
            let mut ret = Sha1([0; 20]);
            &mut ret.0[..].copy_from_slice(bytes);
            Ok(ret)
        }
    }

    pub fn from_thrift(h: thrift::Sha1) -> Result<Self> {
        // Currently this doesn't require consuming b, but hopefully with T26959816 this
        // code will be able to convert a SmallVec directly into an array.
        if h.0.len() != 20 {
            bail!(ErrorKind::InvalidThrift(
                "Sha1".into(),
                format!("wrong length: expected 20, got {}", h.0.len())
            ));
        }
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&h.0[..]);
        Ok(Sha1(arr))
    }

    /// Construct a `Sha1` from an array of 20 bytes.
    #[inline]
    pub const fn from_byte_array(arr: [u8; 20]) -> Sha1 {
        Sha1(arr)
    }

    /// Extract this hash's underlying byte array.
    #[inline]
    pub(crate) fn into_byte_array(self) -> [u8; 20] {
        self.0
    }

    /// Construct a `Sha1` from a hex-encoded `AsciiStr`.
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<Sha1> {
        Self::from_str(s.as_str())
    }

    pub fn to_hex(&self) -> AsciiString {
        let mut v = vec![0; 40];

        // This can only panic if buffer size of Vec isn't correct, which would be
        // a programming error.
        hex_encode(self.as_ref(), &mut v).expect("failed to hex encode");

        unsafe {
            // A hex string is always a pure ASCII string.
            AsciiString::from_ascii_unchecked(v)
        }
    }

    pub fn into_thrift(self) -> thrift::Sha1 {
        // This doesn't need to consume self today, but once T26959816 is implemented it
        // should be possible to do that without copying.
        thrift::Sha1(self.0.to_vec())
    }
}

/// Context for incrementally computing a `Sha1` hash.
#[derive(Clone)]
pub struct Context(sha1::Sha1);

/// Compute the `Sha1` for a slice of bytes.
impl<'a> From<&'a [u8]> for Sha1 {
    fn from(data: &[u8]) -> Sha1 {
        let mut sha1 = sha1::Sha1::new();
        sha1.input(data);

        let mut ret = NULL;
        sha1.result(&mut ret.0[..]);
        ret
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
        if s.len() != 40 {
            bail!(ErrorKind::InvalidSha1Input(
                "need exactly 40 hex digits".into()
            ));
        }

        let mut ret = Sha1([0; 20]);
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
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let mut bytes = [0; 20];
        // The null hash is special, so give it a 5% chance of happening
        if !g.gen_ratio(1, 20) {
            g.fill_bytes(&mut bytes);
        }
        Sha1::from_bytes(&bytes).unwrap()
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        single_shrinker(NULL)
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
        self.0.input(data.as_ref())
    }

    pub fn finish(mut self) -> Sha1 {
        let mut ret = NULL;
        self.0.result(&mut ret.0[..]);
        ret
    }
}

#[cfg(test)]
mod test {
    use quickcheck::{quickcheck, TestResult};
    use std::str::FromStr;

    use super::*;

    #[cfg_attr(rustfmt, rustfmt_skip)]
    const NILHASH: Sha1 = Sha1([0xda, 0x39, 0xa3, 0xee,
                                0x5e, 0x6b, 0x4b, 0x0d,
                                0x32, 0x55, 0xbf, 0xef,
                                0x95, 0x60, 0x18, 0x90,
                                0xaf, 0xd8, 0x07, 0x09]);

    #[test]
    fn test_null() {
        assert_eq!(NULL, Sha1([0_u8; 20]));
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
    fn parse_bad() {
        match Sha1::from_str("") {
            Ok(_) => panic!("unexpected OK - zero len"),
            Err(_) => (),
        };
        match Sha1::from_str("da39a3ee5e6b4b0d3255bfef95601890afd8070") {
            // one char missing
            Ok(_) => panic!("unexpected OK - trunc"),
            Err(_) => (),
        };
        match Sha1::from_str("xda39a3ee5e6b4b0d3255bfef95601890afd8070") {
            // one char bad
            Ok(_) => panic!("unexpected OK - badchar end"),
            Err(_) => (),
        };
        match Sha1::from_str("da39a3ee5e6b4b0d3255bfef95601890afd8070x") {
            // one char bad
            Ok(_) => panic!("unexpected OK - badchar end"),
            Err(_) => (),
        };
        match Sha1::from_str("da39a3ee5e6b4b0d325Xbfef95601890afd80709") {
            // one char missing
            Ok(_) => panic!("unexpected OK - trunc"),
            Err(_) => (),
        };
    }

    #[test]
    fn parse_thrift() {
        let null_thrift = thrift::Sha1(vec![0; 20]);
        assert_eq!(NULL, Sha1::from_thrift(null_thrift.clone()).unwrap());
        assert_eq!(NULL.into_thrift(), null_thrift);

        let nil_thrift = thrift::Sha1(NILHASH.0.to_vec());
        assert_eq!(NILHASH, Sha1::from_thrift(nil_thrift.clone()).unwrap());
        assert_eq!(NILHASH.into_thrift(), nil_thrift);
    }

    #[test]
    fn parse_thrift_bad() {
        Sha1::from_thrift(thrift::Sha1(vec![])).expect_err("unexpected OK - zero len");
        Sha1::from_thrift(thrift::Sha1(vec![0; 19])).expect_err("unexpected OK - too short");
        Sha1::from_thrift(thrift::Sha1(vec![0; 21])).expect_err("unexpected OK - too long");
    }

    quickcheck! {
        fn parse_roundtrip(v: Vec<u8>) -> TestResult {
            if v.len() != 20 {
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
