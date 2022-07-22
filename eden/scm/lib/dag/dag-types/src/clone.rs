/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

use crate::id::Id;
use crate::segment::PreparedFlatSegments;

#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
pub struct CloneData<Name> {
    pub flat_segments: PreparedFlatSegments,
    pub idmap: BTreeMap<Id, Name>,
}

impl<Name> CloneData<Name> {
    pub fn convert_vertex<T, F: Fn(Name) -> T>(self, f: F) -> CloneData<T> {
        let idmap = self.idmap.into_iter().map(|(k, v)| (k, f(v))).collect();
        CloneData {
            flat_segments: self.flat_segments,
            idmap,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Gen;

#[cfg(any(test, feature = "for-tests"))]
impl<Name> Arbitrary for CloneData<Name>
where
    Name: Arbitrary,
{
    fn arbitrary(g: &mut Gen) -> Self {
        let flat_segments = PreparedFlatSegments {
            segments: Arbitrary::arbitrary(g),
        };
        CloneData {
            flat_segments,
            idmap: Arbitrary::arbitrary(g),
        }
    }
}
