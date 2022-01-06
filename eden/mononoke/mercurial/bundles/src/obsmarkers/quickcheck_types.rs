/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use quickcheck::{Arbitrary, Gen};

use super::MetadataEntry;

impl Arbitrary for MetadataEntry {
    fn arbitrary(g: &mut Gen) -> Self {
        let key = String::arbitrary(g);
        let value = String::arbitrary(g);
        Self { key, value }
    }
}
