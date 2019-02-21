// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use serde_derive::{Deserialize, Serialize};

use crate::{key::Key, node::Node};

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
    pub linknode: Node,
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for NodeInfo {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        NodeInfo {
            parents: [Key::arbitrary(g), Key::arbitrary(g)],
            linknode: Node::arbitrary(g),
        }
    }
}
