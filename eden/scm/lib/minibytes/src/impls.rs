/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Implement common traits for [`Bytes`] and [`Text`].

use std::ascii::escape_default;
use std::borrow;
use std::cmp;
use std::fmt;
use std::hash;
use std::ops;

use crate::Bytes;
use crate::BytesOwner;
use crate::Text;
use crate::TextOwner;

impl<T: BytesOwner> From<T> for Bytes {
    fn from(value: T) -> Self {
        Self::from_owner(value)
    }
}

impl From<&'static [u8]> for Bytes {
    fn from(value: &'static [u8]) -> Self {
        Self::from_static(value)
    }
}

impl From<&'static str> for Bytes {
    fn from(value: &'static str) -> Self {
        Self::from_static(value.as_bytes())
    }
}

impl AsRef<[u8]> for Bytes {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl ops::Deref for Bytes {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl hash::Hash for Bytes {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl borrow::Borrow<[u8]> for Bytes {
    fn borrow(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Default for Bytes {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: AsRef<[u8]>> PartialEq<T> for Bytes {
    fn eq(&self, other: &T) -> bool {
        self.as_slice() == other.as_ref()
    }
}

impl Eq for Bytes {}

impl<T: AsRef<[u8]>> PartialOrd<T> for Bytes {
    fn partial_cmp(&self, other: &T) -> Option<cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_ref())
    }
}

impl Ord for Bytes {
    fn cmp(&self, other: &Bytes) -> cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Use `[u8]::escape_ascii` when inherent_ascii_escape is stabilized.
        f.write_str("b\"")?;
        for &byte in self.as_slice() {
            fmt::Display::fmt(&escape_default(byte), f)?;
        }
        f.write_str("\"")?;
        Ok(())
    }
}

impl<T: TextOwner> From<T> for Text {
    fn from(value: T) -> Self {
        Self::from_owner(value)
    }
}

impl From<&'static str> for Text {
    fn from(value: &'static str) -> Self {
        Self::from_static(value)
    }
}

impl AsRef<str> for Text {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_slice()
    }
}

impl ops::Deref for Text {
    type Target = str;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl hash::Hash for Text {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl borrow::Borrow<str> for Text {
    fn borrow(&self) -> &str {
        self.as_slice()
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: AsRef<str>> PartialEq<T> for Text {
    fn eq(&self, other: &T) -> bool {
        self.as_slice() == other.as_ref()
    }
}

impl Eq for Text {}

impl<T: AsRef<str>> PartialOrd<T> for Text {
    fn partial_cmp(&self, other: &T) -> Option<cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_ref())
    }
}

impl Ord for Text {
    fn cmp(&self, other: &Text) -> cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self.as_slice(), f)
    }
}
