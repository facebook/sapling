/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    fmt::{self, Debug, Display},
    io::{self, Read, Write},
};

use failure::{Fail, Fallible as Result};
use serde_derive::{Deserialize, Serialize};

#[cfg(any(test, feature = "for-tests"))]
use rand::RngCore;

#[cfg(any(test, feature = "for-tests"))]
use std::collections::HashSet;

#[derive(Debug, Fail)]
#[fail(display = "HgId Error: {:?}", _0)]
struct HgIdError(String);

const HEX_CHARS: &[u8] = b"0123456789abcdef";

/// A 20-byte identifier, often a hash. Nodes are used to uniquely identify
/// commits, file versions, and many other things.
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
pub struct HgId([u8; HgId::len()]);

/// The nullid (0x00) is used throughout Mercurial to represent "None".
/// (For example, a commit will have a nullid p2, if it has no second parent).
pub const NULL_ID: HgId = HgId([0; HgId::len()]);

impl HgId {
    pub fn null_id() -> &'static Self {
        &NULL_ID
    }

    pub fn is_null(&self) -> bool {
        self == &NULL_ID
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

    pub fn from_byte_array(bytes: [u8; HgId::len()]) -> Self {
        HgId(bytes)
    }

    // Taken from Mononoke
    pub fn from_str(s: &str) -> Result<Self> {
        if s.len() != HgId::hex_len() {
            return Err(HgIdError(format!("invalid string length {:?}", s.len())).into());
        }

        let mut ret = HgId([0u8; HgId::len()]);

        for idx in 0..ret.0.len() {
            ret.0[idx] = match u8::from_str_radix(&s[(idx * 2)..(idx * 2 + 2)], 16) {
                Ok(v) => v,
                Err(_) => return Err(HgIdError(format!("bad digit")).into()),
            }
        }

        Ok(ret)
    }

    pub fn to_hex(&self) -> String {
        let mut v = Vec::with_capacity(HgId::hex_len());
        for &byte in self.as_ref() {
            v.push(HEX_CHARS[(byte >> 4) as usize]);
            v.push(HEX_CHARS[(byte & 0xf) as usize]);
        }

        unsafe { String::from_utf8_unchecked(v) }
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

    quickcheck! {
        fn test_from_slice(hgid: HgId) -> bool {
            hgid == HgId::from_slice(hgid.as_ref()).expect("from_slice")
        }
    }
}
