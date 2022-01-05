/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use type_macros::auto_wire;
use types::HgId;

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullFastForwardRequest {
    #[id(1)]
    pub old_master: HgId,
    #[id(2)]
    pub new_master: HgId,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for PullFastForwardRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        PullFastForwardRequest {
            old_master: HgId::arbitrary(g),
            new_master: HgId::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(WirePullFastForwardRequest);
}
