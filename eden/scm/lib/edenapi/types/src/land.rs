/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::HgId;

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct PushVar {
    #[id(0)]
    pub key: String,

    #[id(1)]
    pub value: String,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct LandStackRequest {
    #[id(0)]
    pub bookmark: String,

    #[id(1)]
    pub head: HgId,

    #[id(2)]
    pub base: HgId,

    #[id(4)]
    pub pushvars: Vec<PushVar>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct LandStackResponse {
    #[id(0)]
    pub new_head: HgId,

    #[id(1)]
    pub old_to_new_hgids: HashMap<HgId, HgId>,
}
