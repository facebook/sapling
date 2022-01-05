/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde_derive::Deserialize;
use serde_derive::Serialize;

use crate::hgid::HgId;
use crate::key::Key;

#[derive(
    Clone,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
pub struct NodeInfo {
    pub parents: [Key; 2],
    pub linknode: HgId,
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Gen;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for NodeInfo {
    fn arbitrary(g: &mut Gen) -> Self {
        NodeInfo {
            parents: [Key::arbitrary(g), Key::arbitrary(g)],
            linknode: HgId::arbitrary(g),
        }
    }
}
