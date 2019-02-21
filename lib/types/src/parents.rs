// Copyright Facebook, Inc. 2019.

use serde_derive::{Deserialize, Serialize};

use crate::node::{Node, NULL_ID};

/// Enum representing a Mercurial node's parents.
///
/// A node may have zero, one, or two parents (referred to as p1 and p2 respectively).
/// Ordinarily, a non-existent parent is denoted by a null hash, consisting of all zeros.
/// A null p1 implies a null p2, so it is invalid for a node to have a p2 without a p1.
///
/// In Rust, these restrictions can be enforced with an enum that makes invalid
/// states unrepresentable.
#[derive(
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
pub enum Parents {
    None,
    One(Node),
    Two(Node, Node),
}

impl Parents {
    /// Construct a new Parents from two potentially null Node hashes.
    /// This function will panic if an invalid combination of Nodes is given --
    /// namely, if p1 is null but p2 is not null.
    pub fn new(p1: Node, p2: Node) -> Self {
        match (p1, p2) {
            (NULL_ID, NULL_ID) => Parents::None,
            (p1, NULL_ID) => Parents::One(p1),
            (NULL_ID, _) => panic!("invalid parents: non-null p2 with null p1"),
            (p1, p2) => Parents::Two(p1, p2),
        }
    }

    /// Convert this Parents into a tuple representation, with non-existent
    /// parents represented by NULL_ID.
    pub fn into_nodes(self) -> (Node, Node) {
        match self {
            Parents::None => (NULL_ID, NULL_ID),
            Parents::One(p1) => (p1, NULL_ID),
            Parents::Two(p1, p2) => (p1, p2),
        }
    }

    pub fn p1(&self) -> Option<&Node> {
        match self {
            Parents::None => None,
            Parents::One(ref p1) => Some(p1),
            Parents::Two(ref p1, _) => Some(p1),
        }
    }

    pub fn p2(&self) -> Option<&Node> {
        match self {
            Parents::None | Parents::One(_) => None,
            Parents::Two(_, ref p2) => Some(p2),
        }
    }
}

impl Default for Parents {
    fn default() -> Self {
        Parents::None
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for Parents {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        match g.next_u64() % 3 {
            0 => Parents::None,
            1 => Parents::One(Node::arbitrary(g)),
            2 => Parents::Two(Node::arbitrary(g), Node::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}
