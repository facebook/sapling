/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use mercurial_types::HgChangesetId;
#[cfg(test)]
use quickcheck_arbitrary_derive::Arbitrary;

/// Wrapper around `HgChangesetId` to force alignment to 8 bytes.
#[derive(Abomonation, Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(test, derive(Arbitrary))]
#[repr(align(8))]
pub struct AlignedHgChangesetId(HgChangesetId);

impl AlignedHgChangesetId {
    pub fn into_inner(self) -> HgChangesetId {
        self.0
    }

    pub fn as_ref(&self) -> &HgChangesetId {
        &self.0
    }
}

impl From<HgChangesetId> for AlignedHgChangesetId {
    fn from(id: HgChangesetId) -> Self {
        Self(id)
    }
}
