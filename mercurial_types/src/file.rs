// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use quickcheck::{single_shrinker, Arbitrary, Gen};

/// Contents of a Mercurial file, stripped of any inline metadata.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FileBytes(pub Bytes);

impl FileBytes {
    pub fn size(&self) -> usize {
        self.0.len()
    }

    /// Whether this starts with a particular string.
    #[inline]
    pub fn starts_with(&self, needle: &[u8]) -> bool {
        self.0.starts_with(needle)
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    pub fn as_bytes(&self) -> &Bytes {
        &self.0
    }
}

impl IntoIterator for FileBytes {
    type Item = <Bytes as IntoIterator>::Item;
    type IntoIter = <Bytes as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Extend<u8> for FileBytes {
    fn extend<T: IntoIterator<Item = u8>>(&mut self, iter: T) {
        self.0.extend(iter)
    }
}

impl Default for FileBytes {
    fn default() -> Self {
        Self(Bytes::default())
    }
}

impl Arbitrary for FileBytes {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        FileBytes(Vec::arbitrary(g).into())
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        single_shrinker(FileBytes(vec![].into()))
    }
}
