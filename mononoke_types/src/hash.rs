// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Debug, Display};
use std::io::Write;
use std::str::FromStr;

use abomonation_derive::Abomonation;
use ascii::{AsciiStr, AsciiString};
use blake2::digest::{Input, VariableOutput};
use blake2::VarBlake2b;
use failure_ext::bail_err;
use faster_hex::{hex_decode, hex_encode};
use heapsize_derive::HeapSizeOf;
use quickcheck::{empty_shrinker, Arbitrary, Gen};
use serde_derive::{Deserialize, Serialize};

use crate::errors::*;
use crate::thrift;

// There is no NULL_HASH for Blake2 hashes. Any places that need a null hash should use an
// Option type, or perhaps a list as desired.

/// Raw BLAKE2b hash.
///
/// Mononoke's internal hashes are based on the BLAKE2b format, used to generate 256-bit (32-byte)
/// hashes.
///
/// This type is not used directly in most cases -- it is only used to build more specific typed
/// hashes.
///
/// For more on BLAKE2b, see https://blake2.net/
#[derive(
    Abomonation,
    Clone,
    Copy,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Serialize,
    Deserialize,
    HeapSizeOf
)]
pub struct Blake2([u8; 32]);

impl Blake2 {
    /// Construct a `Blake2` from an array of 32 bytes containing a
    /// BLAKE2b hash (ie, *not* a hash of the bytes).
    pub fn from_bytes<B: AsRef<[u8]>>(bytes: B) -> Result<Self> {
        let bytes = bytes.as_ref();
        if bytes.len() != 32 {
            bail_err!(ErrorKind::InvalidBlake2Input(
                "need exactly 32 bytes".into()
            ));
        } else {
            let mut ret = Blake2([0; 32]);
            &mut ret.0[..].copy_from_slice(bytes);
            Ok(ret)
        }
    }

    /// Construct a `Blake2` from an array of 32 bytes.
    #[inline]
    pub const fn from_byte_array(arr: [u8; 32]) -> Self {
        Blake2(arr)
    }

    #[inline]
    pub(crate) fn from_thrift(b: thrift::Blake2) -> Result<Self> {
        // Currently this doesn't require consuming b, but hopefully with T26959816 this
        // code will be able to convert a SmallVec directly into an array.
        if b.0.len() != 32 {
            bail_err!(ErrorKind::InvalidThrift(
                "Blake2".into(),
                format!("wrong length: expected 32, got {}", b.0.len())
            ));
        }
        // BigEndian here is matched with `to_thrift` below.
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&b.0[..]);
        Ok(Blake2(arr))
    }

    /// Construct a `Blake2` from a hex-encoded `AsciiStr`.
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<Self> {
        Self::from_str(s.as_str())
    }

    pub fn to_hex(&self) -> AsciiString {
        let mut v = vec![0; 64];

        // This can only panic if buffer size of Vec isn't correct, which would be
        // a programming error.
        hex_encode(self.as_ref(), &mut v).expect("failed to hex encode");

        unsafe {
            // A hex string is always a pure ASCII string.
            AsciiString::from_ascii_unchecked(v)
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::Blake2 {
        // This doesn't need to consume self today, but once T26959816 is implemented it
        // should be possible to do that without copying.
        thrift::Blake2(self.0.to_vec())
    }
}

/// Context for incrementally computing a `Blake2` hash.
#[derive(Clone)]
pub struct Context(VarBlake2b);

impl Context {
    /// Construct a `Context`
    #[inline]
    pub fn new(key: &[u8]) -> Self {
        Context(VarBlake2b::new_keyed(key, 32))
    }

    #[inline]
    pub fn update<T>(&mut self, data: T)
    where
        T: AsRef<[u8]>,
    {
        self.0.input(data.as_ref())
    }

    #[inline]
    pub fn finish(self) -> Blake2 {
        let mut ret = [0u8; 32];
        self.0.variable_result(|res| {
            ret.as_mut()
                .write_all(res)
                .expect("32-byte array must work with 32-byte blake2b");
        });
        Blake2(ret)
    }
}

/// Get a reference to the underlying bytes of a `Blake2`
impl AsRef<[u8]> for Blake2 {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl FromStr for Blake2 {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if s.len() != 64 {
            bail_err!(ErrorKind::InvalidBlake2Input(
                "need exactly 64 hex digits".into()
            ));
        }

        let mut ret = Blake2([0; 32]);
        match hex_decode(s.as_bytes(), &mut ret.0) {
            Ok(_) => Ok(ret),
            Err(_) => bail_err!(ErrorKind::InvalidBlake2Input("bad hex character".into())),
        }
    }
}

impl Display for Blake2 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.to_hex(), fmt)
    }
}

/// Custom `Debug` output for `Blake2` so it prints in hex.
impl Debug for Blake2 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Blake2({})", self)
    }
}

impl Arbitrary for Blake2 {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let mut bytes = [0; 32];
        g.fill_bytes(&mut bytes);
        Blake2(bytes)
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        empty_shrinker()
    }
}

// There is no NULL_HASH for Sha256 hashes. Any places that need a null hash should use an
// Option type, or perhaps a list as desired.

/// Raw SHA256 hash.
///
/// Used for references for blobs in blobstore.
///
#[derive(Abomonation, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
pub struct Sha256([u8; 32]);

impl Sha256 {
    /// Construct a `Sha256` from an array of 32 bytes containing a
    /// Sha256 hash (ie, *not* a hash of the bytes).
    pub fn from_bytes<B: AsRef<[u8]>>(bytes: B) -> Result<Self> {
        let bytes = bytes.as_ref();
        if bytes.len() != 32 {
            bail_err!(ErrorKind::InvalidSha256Input(
                "need exactly 32 bytes".into()
            ));
        } else {
            let mut ret = Sha256([0; 32]);
            &mut ret.0[..].copy_from_slice(bytes);
            Ok(ret)
        }
    }

    /// Construct a `Sha256` from an array of 32 bytes.
    #[inline]
    pub const fn from_byte_array(arr: [u8; 32]) -> Self {
        Sha256(arr)
    }

    pub fn to_hex(&self) -> AsciiString {
        let mut v = vec![0; 64];

        // This can only panic if buffer size of Vec isn't correct, which would be
        // a programming error.
        hex_encode(self.as_ref(), &mut v).expect("failed to hex encode");

        unsafe {
            // A hex string is always a pure ASCII string.
            AsciiString::from_ascii_unchecked(v)
        }
    }

    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<Self> {
        Self::from_str(s.as_str())
    }
}

impl FromStr for Sha256 {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if s.len() != 64 {
            bail_err!(ErrorKind::InvalidSha256Input(
                "must be 64 hex digits".into()
            ));
        }

        let mut ret = Sha256([0; 32]);
        match hex_decode(s.as_bytes(), &mut ret.0) {
            Ok(_) => Ok(ret),
            Err(_) => bail_err!(ErrorKind::InvalidSha256Input("bad hex character".into())),
        }
    }
}

/// Get a reference to the underlying bytes of a `Blake2`
impl AsRef<[u8]> for Sha256 {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

/// Custom `Debug` output for `Sha256` so it prints in hex.
impl Debug for Sha256 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Sha256({})", self.to_hex())
    }
}

/// Custom `Debug` output for `Sha256` so it prints in hex.
impl Display for Sha256 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.to_hex(), fmt)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::{quickcheck, TestResult};

    // NULL is not exposed because no production code should use it.
    const NULL: Blake2 = Blake2([0; 32]);

    // This hash is from https://asecuritysite.com/encryption/blake.
    #[cfg_attr(rustfmt, rustfmt_skip)]
    const NILHASH: Blake2 = Blake2([0x0e, 0x57, 0x51, 0xc0,
                                    0x26, 0xe5, 0x43, 0xb2,
                                    0xe8, 0xab, 0x2e, 0xb0,
                                    0x60, 0x99, 0xda, 0xa1,
                                    0xd1, 0xe5, 0xdf, 0x47,
                                    0x77, 0x8f, 0x77, 0x87,
                                    0xfa, 0xab, 0x45, 0xcd,
                                    0xf1, 0x2f, 0xe3, 0xa8]);

    #[test]
    fn test_nil() {
        let context = Context::new(b"");
        let nil = context.finish();
        assert_eq!(nil, NILHASH);
    }

    #[test]
    fn parse_ok() {
        assert_eq!(
            NULL,
            Blake2::from_str("0000000000000000000000000000000000000000000000000000000000000000")
                .unwrap()
        );
        assert_eq!(
            NILHASH,
            Blake2::from_str("0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8")
                .unwrap()
        );
        assert_eq!(
            NILHASH,
            Blake2::from_str("0E5751C026E543B2E8AB2EB06099DAA1D1E5DF47778F7787FAAB45CDF12FE3A8")
                .unwrap()
        );
    }

    #[test]
    fn parse_thrift() {
        let null_thrift = thrift::Blake2(vec![0; 32]);
        assert_eq!(NULL, Blake2::from_thrift(null_thrift.clone()).unwrap());
        assert_eq!(NULL.into_thrift(), null_thrift);

        let nil_thrift = thrift::Blake2(NILHASH.0.to_vec());
        assert_eq!(NILHASH, Blake2::from_thrift(nil_thrift.clone()).unwrap());
        assert_eq!(NILHASH.into_thrift(), nil_thrift);
    }

    #[test]
    fn test_display() {
        assert_eq!(
            format!("{}", NULL),
            "0000000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            format!("{}", NILHASH),
            "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8"
        );
    }

    #[test]
    fn parse_bad() {
        Blake2::from_str("").expect_err("unexpected OK - zero len");
        Blake2::from_str("0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a")
            .expect_err("unexpected OK - trunc");
        Blake2::from_str("xe5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a")
            .expect_err("unexpected OK - badchar beginning");
        Blake2::from_str("0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3x")
            .expect_err("unexpected OK - badchar end");
        Blake2::from_str("0e5751c026e543b2e8ab2eb06099daa1d1x5df47778f7787faab45cdf12fe3a")
            .expect_err("unexpected OK - badchar middle");
    }

    #[test]
    fn parse_thrift_bad() {
        Blake2::from_thrift(thrift::Blake2(vec![])).expect_err("unexpected OK - zero len");
        Blake2::from_thrift(thrift::Blake2(vec![0; 31])).expect_err("unexpected OK - too short");
        Blake2::from_thrift(thrift::Blake2(vec![0; 33])).expect_err("unexpected Ok - too long");
    }

    quickcheck! {
        fn parse_roundtrip(v: Vec<u8>) -> TestResult {
            if v.len() != 32 {
                return TestResult::discard()
            }
            let h = Blake2::from_bytes(v).unwrap();
            let s = format!("{}", h);
            let sh = s.parse().unwrap();

            TestResult::from_bool(h == sh)
        }

        fn to_hex_roundtrip(h: Blake2) -> bool {
            let v = h.to_hex();
            let sh = Blake2::from_ascii_str(&v).unwrap();
            h == sh
        }

        fn thrift_roundtrip(h: Blake2) -> bool {
            let v = h.into_thrift();
            let sh = Blake2::from_thrift(v).expect("converting a valid Thrift structure should always work");
            h == sh
        }
    }
}
