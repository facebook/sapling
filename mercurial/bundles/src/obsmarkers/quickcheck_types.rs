/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use quickcheck::{Arbitrary, Gen};

use super::MetadataEntry;

impl Arbitrary for MetadataEntry {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let key = String::arbitrary(g);
        let value = String::arbitrary(g);
        Self { key, value }
    }
}
