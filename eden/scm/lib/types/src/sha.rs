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
