/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use quickcheck::single_shrinker;
use quickcheck::Arbitrary;
use quickcheck::Gen;

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

impl Default for FileBytes {
    fn default() -> Self {
        Self(Bytes::default())
    }
}

impl Arbitrary for FileBytes {
    fn arbitrary(g: &mut Gen) -> Self {
        FileBytes(Vec::arbitrary(g).into())
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        single_shrinker(FileBytes(vec![].into()))
    }
}
