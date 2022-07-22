/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # id
//!
//! Defines types around [`Id`].

use std::fmt;
use std::io;
use std::ops;

use abomonation_derive::Abomonation;
pub use minibytes::Bytes;
use serde::Deserialize;
use serde::Serialize;

/// An integer [`Id`] representing a node in the graph.
/// [`Id`]s are topologically sorted.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(Serialize, Deserialize, Abomonation)]
pub struct Id(pub u64);

/// Name of a vertex in the graph.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VertexName(pub Bytes);

impl AsRef<[u8]> for VertexName {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl VertexName {
    pub fn to_hex(&self) -> String {
        const HEX_CHARS: &[u8] = b"0123456789abcdef";
        let mut v = Vec::with_capacity(self.0.len() * 2);
        for &byte in self.as_ref() {
            v.push(HEX_CHARS[(byte >> 4) as usize]);
            v.push(HEX_CHARS[(byte & 0xf) as usize]);
        }
        unsafe { String::from_utf8_unchecked(v) }
    }

    /// Convert from hex.
    ///
    /// If `len(hex)` is an odd number, hex + '0' will be used.
    pub fn from_hex(hex: &[u8]) -> io::Result<Self> {
        let mut bytes = vec![0u8; (hex.len() + 1) / 2];
        for (i, byte) in hex.iter().enumerate() {
            let value = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{:?} is not a hex character", *byte as char),
                    ));
                }
            };
            if i & 1 == 0 {
                bytes[i / 2] |= value << 4;
            } else {
                bytes[i / 2] |= value;
            }
        }
        Ok(VertexName(Bytes::from(bytes)))
    }

    pub fn copy_from(value: &[u8]) -> Self {
        Self(value.to_vec().into())
    }
}

impl<T> From<T> for VertexName
where
    Bytes: From<T>,
{
    fn from(value: T) -> Self {
        Self(Bytes::from(value))
    }
}

impl fmt::Debug for VertexName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.0.len() >= 4 || !looks_like_ascii_identifier(self.as_ref()) {
            // Use hex format for long names (ex. binary commit hashes).
            let hex = self.to_hex();
            // Truncate to specified width (ex. '{:#.12}').
            if let Some(width) = f.precision() {
                let truncated = hex.get(..width).unwrap_or(&hex);
                f.write_str(truncated)
            } else {
                f.write_str(&hex)
            }
        } else {
            // Do not use hex if it looks like an ASCII identifier.
            match std::str::from_utf8(self.as_ref()) {
                Ok(s) => write!(f, "{}", s),
                Err(_) => write!(f, "{}", self.to_hex()),
            }
        }
    }
}

fn looks_like_ascii_identifier(bytes: &[u8]) -> bool {
    let mut iter = bytes.iter().copied();
    if !(iter.next().unwrap_or(b'\0') as char).is_ascii_alphabetic() {
        return false;
    }
    iter.all(|b| b.is_ascii_alphanumeric())
}

/// An integer that separates distinct groups of [`Id`]s.
///
/// This can be seen as a way to pre-allocate consecutive integers
/// for one group to make segments less fragmented.
///
/// `(Group, Id)` are also topologically sorted.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(Serialize, Deserialize)]
pub struct Group(pub usize);

impl Group {
    /// The "master" group. `ancestors(master)`.
    /// - Expected to have most of the commits in a repo.
    /// - Expected to be free from fragmentation. In other words,
    ///   `ancestors(master)` can be represented in a single Span.
    pub const MASTER: Self = Self(0);

    /// The "non-master" group.
    /// - Anything not in `ancestors(master)`. For example, public release
    ///   branches, local feature branches.
    /// - Expected to have multiple heads. In other words, is fragmented.
    /// - Expected to be sparse referred. For example, the "visible heads"
    ///   will refer to a bounded subset in this group.
    pub const NON_MASTER: Self = Self(1);

    pub const ALL: [Self; 2] = [Self::MASTER, Self::NON_MASTER];

    pub const COUNT: usize = Self::ALL.len();

    // 1 byte for Group so it's easier to remove everything in a group.
    pub const BITS: u32 = 8;
    pub const BYTES: usize = 1;

    /// The first [`Id`] in this group.
    pub const fn min_id(self) -> Id {
        Id((self.0 as u64) << (64 - Self::BITS))
    }

    /// The maximum [`Id`] in this group.
    pub const fn max_id(self) -> Id {
        Id(self.min_id().0 + ((1u64 << (64 - Self::BITS)) - 1))
    }

    /// Convert to array.
    pub const fn bytes(self) -> [u8; 1] {
        [self.0 as u8]
    }

    /// Convert to hex array.
    pub fn hex_bytes(self) -> [u8; 2] {
        if self.0 < 10 {
            [b'0', b'0' + (self.0 as u8)]
        } else {
            unreachable!()
        }
    }
}

impl Id {
    /// The [`Group`] of an Id.
    pub fn group(self) -> Group {
        let group = (self.0 >> (64 - Group::BITS)) as usize;
        debug_assert!(group < Group::COUNT);
        Group(group)
    }

    /// Similar to `self..=other`.
    pub fn to(self, other: Id) -> IdIter {
        IdIter {
            current: self,
            end: other,
        }
    }

    /// Convert to a byte array. Useful for indexedlog range query.
    pub fn to_bytearray(self) -> [u8; 8] {
        // The field can be used for index range query. So it has to be BE.
        unsafe { std::mem::transmute(self.0.to_be()) }
    }

    /// Similar to `to_bytearray`, but insert a `prefix` at the head.
    /// Useful for segment queries where `level` is the `prefix`.
    pub fn to_prefixed_bytearray(self, prefix: u8) -> [u8; 9] {
        let a = self.to_bytearray();
        [prefix, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]]
    }

    pub const MAX: Self = Group::ALL[Group::COUNT - 1].max_id();
    pub const MIN: Self = Group::ALL[0].min_id();
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let group = self.group();
        if group == Group::NON_MASTER {
            write!(f, "N")?;
        }
        write!(f, "{}", self.0 - group.min_id().0)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Group::MASTER => write!(f, "Group Master"),
            Group::NON_MASTER => write!(f, "Group Non-Master"),
            _ => write!(f, "Group {}", self.0),
        }
    }
}

impl ops::Add<u64> for Id {
    type Output = Id;

    fn add(self, other: u64) -> Self {
        Self(self.0 + other)
    }
}

impl ops::Sub<u64> for Id {
    type Output = Id;

    fn sub(self, other: u64) -> Self {
        Self(self.0 - other)
    }
}

// Consider replacing this with iter::Step once it's stable.
pub struct IdIter {
    current: Id,
    end: Id,
}

impl Iterator for IdIter {
    type Item = Id;

    fn next(&mut self) -> Option<Id> {
        if self.current > self.end {
            None
        } else {
            let result = self.current;
            self.current = self.current + 1;
            Some(result)
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Gen;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for Id {
    fn arbitrary(g: &mut Gen) -> Self {
        let group = Group((u32::arbitrary(g) & 1) as usize);
        group.min_id() + u64::arbitrary(g) % (group.max_id().0 - group.min_id().0)
    }
}

#[cfg(feature = "for-tests")]
impl std::cmp::PartialEq<u64> for Id {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;

    use super::*;

    #[test]
    fn test_vertex_from_hex_odd() {
        let vertex = VertexName::from_hex(b"a").unwrap();
        let vertex2 = VertexName::from_hex(b"a0").unwrap();
        assert_eq!(vertex, vertex2);
        assert_eq!(vertex.to_hex(), "a0");
    }

    quickcheck! {
        fn test_vertex_hex_roundtrip(slice: Vec<u8>) -> bool {
            let vertex = VertexName::from(slice);
            let hex = vertex.to_hex();
            let vertex2 = VertexName::from_hex(hex.as_bytes()).unwrap();
            vertex2 == vertex
        }
    }
}
