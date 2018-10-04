// Copyright Facebook, Inc. 2018
use errors::Result;
use std::fmt::{self, Display};

#[cfg(any(test, feature = "for-tests"))]
use rand::RngCore;

#[derive(Debug, Fail)]
#[fail(display = "Node Error: {:?}", _0)]
struct NodeError(String);

const NODE_LEN: usize = 20;
const HEX_CHARS: &[u8] = b"0123456789abcdef";

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

    // Taken from Mononoke
    pub fn from_str(s: &str) -> Result<Self> {
        if s.len() != 40 {
            return Err(NodeError(format!("invalid string length {:?}", s.len())).into());
        }

        let mut ret = Node([0u8; 20]);

        for idx in 0..ret.0.len() {
            ret.0[idx] = match u8::from_str_radix(&s[(idx * 2)..(idx * 2 + 2)], 16) {
                Ok(v) => v,
                Err(_) => return Err(NodeError(format!("bad digit")).into()),
            }
        }

        Ok(ret)
    }

    pub fn to_hex(&self) -> String {
        let mut v = Vec::with_capacity(40);
        for &byte in self.as_ref() {
            v.push(HEX_CHARS[(byte >> 4) as usize]);
            v.push(HEX_CHARS[(byte & 0xf) as usize]);
        }

        unsafe { String::from_utf8_unchecked(v) }
    }

    #[cfg(any(test, feature = "for-tests"))]
    pub fn random(rng: &mut RngCore) -> Self {
        let mut bytes = [0; 20];
        rng.fill_bytes(&mut bytes);
        loop {
            let node = Node::from(&bytes);
            if !node.is_null() {
                return node;
            }
        }
    }
}

impl Display for Node {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.to_hex(), fmt)
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

#[cfg(any(test, feature = "for-tests"))]
use quickcheck;

#[cfg(any(test, feature = "for-tests"))]
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
