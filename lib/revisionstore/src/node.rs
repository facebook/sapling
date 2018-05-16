// Copyright Facebook, Inc. 2018

/// A 20-byte identifier, often a hash. Nodes are used to uniquely identify
/// commits, file versions, and many other things.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Node([u8; 20]);

/// The nullid (0x00) is used throughout Mercurial to represent "None".
/// (For example, a commit will have a nullid p2, if it has no second parent).
pub const NULL_ID: Node = Node([0; 20]);

impl Node {
    pub fn is_null(&self) -> bool {
        self == &NULL_ID
    }
}

impl Default for Node {
    fn default() -> Node {
        NULL_ID
    }
}

impl<'a> From<&'a [u8; 20]> for Node {
    fn from(bytes: &[u8; 20]) -> Node {
        Node(bytes.clone())
    }
}

impl AsRef<[u8]> for Node {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[cfg(test)]
use quickcheck;

#[cfg(test)]
impl quickcheck::Arbitrary for Node {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let mut bytes = [0u8; 20];
        g.fill_bytes(&mut bytes);
        Node::from(&bytes)
    }
}
