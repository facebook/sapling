/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use type_macros::auto_wire;
use types::HgId;

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    any(test, feature = "for-tests"),
    derive(quickcheck_arbitrary_derive::Arbitrary)
)]
pub struct PullFastForwardRequest {
    #[id(1)]
    pub old_master: HgId,
    #[id(2)]
    pub new_master: HgId,
}

/// Pull `missing % common` in the master group.
#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    any(test, feature = "for-tests"),
    derive(quickcheck_arbitrary_derive::Arbitrary)
)]
pub struct PullLazyRequest {
    #[id(1)]
    pub common: Vec<HgId>,
    #[id(2)]
    pub missing: Vec<HgId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(WirePullFastForwardRequest);
    auto_wire_tests!(WirePullLazyRequest);
}
