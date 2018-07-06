// Copyright Facebook, Inc. 2018
use error::Result;

#[cfg(test)]
use rand::RngCore;
#[cfg(test)]
use rand::os::OsRng;

#[derive(Debug, Fail)]
#[fail(display = "Node Error: {:?}", _0)]
struct NodeError(String);

const NODE_LEN: usize = 20;

/// A 20-byte identifier, often a hash. Nodes are used to uniquely identify
/// commits, file versions, and many other things.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Node([u8; 20]);

/// The nullid (0x00) is used throughout Mercurial to represent "None".
/// (For example, a commit will have a nullid p2, if it has no second parent).
pub const NULL_ID: Node = Node([0; 20]);

impl Node {
    pub fn null_id() -> &'static Self {
        &NULL_ID
    }

    pub fn is_null(&self) -> bool {
        self == &NULL_ID
    }

    pub fn len() -> usize {
        20
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != NODE_LEN {
            return Err(NodeError(format!("invalid node length {:?}", bytes.len())).into());
        }

        let mut fixed_bytes = [0u8; 20];
        fixed_bytes.copy_from_slice(bytes);
        Ok(Node(fixed_bytes))
    }

    #[cfg(test)]
    pub fn random() -> Self {
        let mut bytes = [0; 20];
        OsRng::new().unwrap().fill_bytes(&mut bytes);
        Node::from(&bytes)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incorrect_length() {
        Node::from_slice(&[0u8; 25]).expect_err("bad slice length");
    }

    quickcheck! {
        fn test_from_slice(node: Node) -> bool {
            node == Node::from_slice(node.as_ref()).expect("from_slice")
        }
    }
}
