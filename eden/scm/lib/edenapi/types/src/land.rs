/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::collections::HashMap;
use type_macros::auto_wire;
use types::HgId;

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct PushVar {
    #[id(0)]
    pub key: String,

    #[id(1)]
    pub value: String,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for PushVar {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        PushVar {
            key: Arbitrary::arbitrary(g),
            value: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
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

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for LandStackRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        LandStackRequest {
            bookmark: Arbitrary::arbitrary(g),
            head: Arbitrary::arbitrary(g),
            base: Arbitrary::arbitrary(g),
            pushvars: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct LandStackResponse {
    #[id(0)]
    pub new_head: HgId,

    #[id(1)]
    pub old_to_new_hgids: HashMap<HgId, HgId>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for LandStackResponse {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        LandStackResponse {
            new_head: Arbitrary::arbitrary(g),
            old_to_new_hgids: Arbitrary::arbitrary(g),
        }
    }
}
