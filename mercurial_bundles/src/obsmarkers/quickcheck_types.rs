// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use quickcheck::{Arbitrary, Gen};

use super::MetadataEntry;

impl Arbitrary for MetadataEntry {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let key = String::arbitrary(g);
        let value = String::arbitrary(g);
        Self { key, value }
    }
}
