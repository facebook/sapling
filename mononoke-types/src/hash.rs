// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Debug, Display};
use std::str::FromStr;

use ascii::{AsciiStr, AsciiString};
use blake2::Blake2b;
use blake2::digest::{Input, VariableOutput};
use quickcheck::{single_shrinker, Arbitrary, Gen};

use errors::*;

pub const NULL: Blake2 = Blake2([0; 32]);

/// Raw BLAKE2b hash.
///
/// Mononoke's internal hashes are based on the BLAKE2b format, used to generate 256-bit (32-byte)
/// hashes.
///
/// This type is not used directly in most cases -- it is only used to build more specific typed
/// hashes.
///
/// For more on BLAKE2b, see https://blake2.net/
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
pub struct Blake2([u8; 32]);

const HEX_CHARS: &[u8] = b"0123456789abcdef";

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

    /// Construct a `Blake2` from a hex-encoded `AsciiStr`.
    #[inline]
    pub fn from_ascii_str(s: &AsciiStr) -> Result<Self> {
        Self::from_str(s.as_str())
    }

    pub fn to_hex(&self) -> AsciiString {
        let mut v = Vec::with_capacity(64);
        for &byte in self.as_ref() {
            v.push(HEX_CHARS[(byte >> 4) as usize]);
            v.push(HEX_CHARS[(byte & 0xf) as usize]);
        }

        unsafe {
            // A hex string is always a pure ASCII string.
            AsciiString::from_ascii_unchecked(v)
        }
    }
}

/// Context for incrementally computing a `Sha1` hash.
#[derive(Clone)]
pub struct Context(Blake2b);

impl Context {
    /// Construct a `Context`
    pub fn new() -> Self {
        Context(Blake2b::new(32).expect("blake2b must support 32 byte outputs"))
    }

    pub fn update<T>(&mut self, data: T)
    where
        T: AsRef<[u8]>,
    {
        self.0.process(data.as_ref())
    }

    pub fn finish(self) -> Blake2 {
        let mut ret = NULL;
        self.0
            .variable_result(&mut ret.0[..])
            .expect("32-byte array must work with 32-byte blake2b");
        ret
    }
}

/// Compute the `Blake2` for a slice of bytes.
impl<'a> From<&'a [u8]> for Blake2 {
    fn from(data: &[u8]) -> Blake2 {
        let mut context = Context::new();
        context.update(data);
        context.finish()
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
        if s.len() < 64 {
            bail_err!(ErrorKind::InvalidBlake2Input(
                "need at least 64 hex digits".into()
            ));
        }

        let mut ret = Blake2([0; 32]);

        for idx in 0..ret.0.len() {
            ret.0[idx] = match u8::from_str_radix(&s[(idx * 2)..(idx * 2 + 2)], 16) {
                Ok(v) => v,
                Err(_) => bail_err!(ErrorKind::InvalidBlake2Input("bad digit".into())),
            };
        }

        Ok(ret)
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
        // The null hash is special, so give it a 5% chance of happening
        if !g.gen_weighted_bool(20) {
            g.fill_bytes(&mut bytes);
        }
        Blake2(bytes)
    }

    fn shrink(&self) -> Box<Iterator<Item = Self>> {
        single_shrinker(NULL)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use quickcheck::TestResult;

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
    fn test_null() {
        assert_eq!(NULL, Blake2([0_u8; 32]));
    }

    #[test]
    fn test_nil() {
        let nil = Blake2::from(&[][..]);
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
    }
}
