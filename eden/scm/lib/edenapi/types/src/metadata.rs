/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde_derive::{Deserialize, Serialize};

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

/// Directory entry metadata
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryMetadata {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryMetadataRequest {}

/// File entry metadata
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadata {
    pub revisionstore_flags: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadataRequest {
    pub with_revisionstore_flags: bool,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for DirectoryMetadata {
    fn arbitrary<G: quickcheck::Gen>(_g: &mut G) -> Self {
        Self {}
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for DirectoryMetadataRequest {
    fn arbitrary<G: quickcheck::Gen>(_g: &mut G) -> Self {
        Self {}
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileMetadata {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            revisionstore_flags: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileMetadataRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            with_revisionstore_flags: Arbitrary::arbitrary(g),
        }
    }
}
