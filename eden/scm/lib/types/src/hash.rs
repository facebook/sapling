/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use serde::de::Error;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde_bytes::ByteBuf;
use thiserror::Error;

/// An abstracted hash type.
///
/// To use this type, provide the name and length information.
/// See `HgId` for example.
pub struct AbstractHashType<I, const N: usize>([u8; N], PhantomData<I>);

/// Describe information about a hash type.
pub trait HashTypeInfo {
    /// The name of the hash type.
    const HASH_TYPE_NAME: &'static str;
}

#[derive(Debug, Error)]
#[error("expect {0} bytes but got {1}")]
pub struct LengthMismatchError(usize, usize);

#[derive(Debug, Error)]
#[error("{0:?} is not a {1}-byte hex string")]
pub struct HexError(Vec<u8>, usize);

// It's unfortunate that I and N are separate parameters. But that's
// limitation by rustc: array length cannot refer to generic types
// (https://github.com/rust-lang/rust/issues/43408), and `&str` cannot
// be used as a const genric, as of rust 1.54.

impl<'de, I, const N: usize> Deserialize<'de> for AbstractHashType<I, N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // ByteBuf supports both list and bytes.
        let bytes: ByteBuf = serde_bytes::deserialize(deserializer)?;
        let bytes = bytes.as_ref();
        // Compatible with hex.
        if bytes.len() == Self::hex_len() {
            Self::from_hex(bytes).map_err(|e| {
                let msg = format!("invalid HgId: {} ({:?})", e, bytes);
                D::Error::custom(msg)
            })
        } else {
            Self::from_slice(bytes).map_err(|e| {
                let msg = format!("invalid HgId: {} ({:?})", e, bytes);
                D::Error::custom(msg)
            })
        }
    }
}

impl<I, const N: usize> Serialize for AbstractHashType<I, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

static HEXIFY_LOOKUP_TABLE: [i8; 256] = [
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, -1, -1, -1, -1, -1, -1, 0, 10, 11, 12, 13, 14, 15, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 0, 10,
    11, 12, 13, 14, 15, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
];

#[inline]
fn hexify(nibble: u8, hex: &[u8]) -> Result<u8, HexError> {
    let res = HEXIFY_LOOKUP_TABLE[nibble as usize];
    if res < 0 {
        Err(HexError(hex.to_vec(), hex.len()))
    } else {
        Ok(res as u8)
    }
}

impl<I, const N: usize> AbstractHashType<I, N> {
    pub const fn len() -> usize {
        N
    }

    pub const fn hex_len() -> usize {
        N * 2
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, LengthMismatchError> {
        if bytes.len() != Self::len() {
            return Err(LengthMismatchError(Self::len(), bytes.len()));
        }

        let mut fixed_bytes = [0u8; N];
        fixed_bytes.copy_from_slice(bytes);
        Ok(Self(fixed_bytes, PhantomData))
    }

    pub const fn from_byte_array(bytes: [u8; N]) -> Self {
        Self(bytes, PhantomData)
    }

    pub fn into_byte_array(self) -> [u8; N] {
        self.0
    }

    pub fn to_hex(&self) -> String {
        to_hex(self.0.as_ref())
    }

    /// Convert the hex string to a binary hash.
    ///
    /// Note: this function is performance sensitive, please benchmark it carefully while changing
    /// it.
    pub fn from_hex(hex: &[u8]) -> Result<Self, HexError> {
        if hex.len() != Self::hex_len() {
            return Err(HexError(hex.to_vec(), Self::hex_len()));
        }
        let mut bytes = [0u8; N];
        for (i, chunk) in hex.chunks_exact(2).enumerate() {
            let high = hexify(chunk[0], hex)?;
            let low = hexify(chunk[1], hex)?;
            bytes[i] = (high << 4) | low;
        }
        Ok(Self::from_byte_array(bytes))
    }
}

impl<I, const N: usize> fmt::Display for AbstractHashType<I, N> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str(&self.to_hex())
    }
}

impl<I: HashTypeInfo, const N: usize> fmt::Debug for AbstractHashType<I, N> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}({:?})", I::HASH_TYPE_NAME, &self.to_hex())
    }
}

impl<I, const N: usize> AsRef<[u8]> for AbstractHashType<I, N> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<I, const N: usize> From<[u8; N]> for AbstractHashType<I, N> {
    fn from(bytes: [u8; N]) -> Self {
        Self::from_byte_array(bytes)
    }
}

impl<I, const N: usize> From<AbstractHashType<I, N>> for [u8; N] {
    fn from(id: AbstractHashType<I, N>) -> Self {
        id.into_byte_array()
    }
}

impl<I, const N: usize> FromStr for AbstractHashType<I, N> {
    type Err = HexError;

    fn from_str(s: &str) -> Result<Self, HexError> {
        Self::from_hex(s.as_bytes())
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl<I: 'static, const N: usize> quickcheck::Arbitrary for AbstractHashType<I, N> {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let mut bytes = [0u8; N];
        for b in bytes.iter_mut() {
            *b = u8::arbitrary(g);
        }
        Self::from_byte_array(bytes)
    }
}

// Boilerplate for common traits.
// These would ideally be just `#[derive(Default, Eq, PartialEq, ...)]`. However,
// using `#[derive(...)]` would put constraints on type parameter `I` undesirably.

impl<I, const N: usize> Default for AbstractHashType<I, N> {
    fn default() -> Self {
        Self([0; N], PhantomData)
    }
}

impl<I, const N: usize> PartialEq<Self> for AbstractHashType<I, N> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<I, const N: usize> Eq for AbstractHashType<I, N> {}

impl<I, const N: usize> PartialOrd<Self> for AbstractHashType<I, N> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<I, const N: usize> Ord for AbstractHashType<I, N> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<I, const N: usize> std::hash::Hash for AbstractHashType<I, N> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<I, const N: usize> Clone for AbstractHashType<I, N> {
    fn clone(&self) -> Self {
        Self(self.0, PhantomData)
    }
}

impl<I, const N: usize> Copy for AbstractHashType<I, N> {}

// Utilities

pub fn to_hex(slice: &[u8]) -> String {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut v = Vec::with_capacity(slice.len() * 2);
    for &byte in slice {
        v.push(HEX_CHARS[(byte >> 4) as usize]);
        v.push(HEX_CHARS[(byte & 0xf) as usize]);
    }
    unsafe { String::from_utf8_unchecked(v) }
}
