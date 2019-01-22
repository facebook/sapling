// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Fallible;

use types::node::Node;

use std::collections::HashMap;

use key::Key;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeInfo {
    pub parents: [Key; 2],
    pub linknode: Node,
}

pub type Ancestors = HashMap<Key, NodeInfo>;

pub trait HistoryStore {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors>;
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>>;
    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo>;
}

#[cfg(test)]
use quickcheck;

#[cfg(test)]
impl quickcheck::Arbitrary for NodeInfo {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        NodeInfo {
            parents: [Key::arbitrary(g), Key::arbitrary(g)],
            linknode: Node::arbitrary(g),
        }
    }
}
