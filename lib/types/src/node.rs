// Copyright Facebook, Inc. 2018
use std::{
    fmt::{self, Debug, Display},
    io::{self, Read, Write},
};

use failure::{Fail, Fallible};
use serde_derive::{Deserialize, Serialize};

#[cfg(any(test, feature = "for-tests"))]
use rand::RngCore;

#[cfg(any(test, feature = "for-tests"))]
use std::collections::HashSet;

#[derive(Debug, Fail)]
#[fail(display = "Node Error: {:?}", _0)]
struct NodeError(String);

const HEX_CHARS: &[u8] = b"0123456789abcdef";

/// A 20-byte identifier, often a hash. Nodes are used to uniquely identify
/// commits, file versions, and many other things.
#[derive(
    Clone,
    Copy,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
pub struct Node([u8; Node::len()]);

/// The nullid (0x00) is used throughout Mercurial to represent "None".
/// (For example, a commit will have a nullid p2, if it has no second parent).
pub const NULL_ID: Node = Node([0; Node::len()]);

impl Node {
    pub fn null_id() -> &'static Self {
        &NULL_ID
    }

    pub fn is_null(&self) -> bool {
        self == &NULL_ID
    }

    pub const fn len() -> usize {
        20
    }

    pub const fn hex_len() -> usize {
        40
    }

    pub fn from_slice(bytes: &[u8]) -> Fallible<Self> {
        if bytes.len() != Node::len() {
            return Err(NodeError(format!("invalid node length {:?}", bytes.len())).into());
        }

        let mut fixed_bytes = [0u8; Node::len()];
        fixed_bytes.copy_from_slice(bytes);
        Ok(Node(fixed_bytes))
    }

    pub fn from_byte_array(bytes: [u8; Node::len()]) -> Self {
        Node(bytes)
    }

    // Taken from Mononoke
    pub fn from_str(s: &str) -> Fallible<Self> {
        if s.len() != Node::hex_len() {
            return Err(NodeError(format!("invalid string length {:?}", s.len())).into());
        }

        let mut ret = Node([0u8; Node::len()]);

        for idx in 0..ret.0.len() {
            ret.0[idx] = match u8::from_str_radix(&s[(idx * 2)..(idx * 2 + 2)], 16) {
                Ok(v) => v,
                Err(_) => return Err(NodeError(format!("bad digit")).into()),
            }
        }

        Ok(ret)
    }

    pub fn to_hex(&self) -> String {
        let mut v = Vec::with_capacity(Node::hex_len());
        for &byte in self.as_ref() {
            v.push(HEX_CHARS[(byte >> 4) as usize]);
            v.push(HEX_CHARS[(byte & 0xf) as usize]);
        }

        unsafe { String::from_utf8_unchecked(v) }
    }

    #[cfg(any(test, feature = "for-tests"))]
    pub fn random(rng: &mut dyn RngCore) -> Self {
        let mut bytes = [0; Node::len()];
        rng.fill_bytes(&mut bytes);
        loop {
            let node = Node::from(&bytes);
            if !node.is_null() {
                return node;
            }
        }
    }

    #[cfg(any(test, feature = "for-tests"))]
    pub fn random_distinct(rng: &mut dyn RngCore, count: usize) -> Vec<Self> {
        let mut nodes = Vec::new();
        let mut nodeset = HashSet::new();
        while nodes.len() < count {
            let node = Node::random(rng);
            if !nodeset.contains(&node) {
                nodeset.insert(node.clone());
                nodes.push(node);
            }
        }
        nodes
    }
}

impl Display for Node {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.to_hex(), fmt)
    }
}

impl Debug for Node {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Node({:?})", &self.to_hex())
    }
}

impl Default for Node {
    fn default() -> Node {
        NULL_ID
    }
}

impl<'a> From<&'a [u8; Node::len()]> for Node {
    fn from(bytes: &[u8; Node::len()]) -> Node {
        Node(bytes.clone())
    }
}

impl AsRef<[u8]> for Node {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub trait WriteNodeExt {
    /// Write a ``Node`` directly to a stream.
    ///
    /// # Examples
    ///
    /// ```
    /// use types::node::{Node, WriteNodeExt};
    /// let mut v = vec![];
    ///
    /// let n = Node::null_id();
    /// v.write_node(&n).expect("writing a node to a vec should work");
    ///
    /// assert_eq!(v, vec![0; Node::len()]);
    /// ```
    fn write_node(&mut self, value: &Node) -> io::Result<()>;
}

impl<W: Write + ?Sized> WriteNodeExt for W {
    fn write_node(&mut self, value: &Node) -> io::Result<()> {
        self.write_all(&value.0)
    }
}

pub trait ReadNodeExt {
    /// Read a ``Node`` directly from a stream.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    /// use types::node::{Node, ReadNodeExt};
    /// let mut v = vec![0; Node::len()];
    /// let mut c = Cursor::new(v);
    ///
    /// let n = c.read_node().expect("reading a node from a vec should work");
    ///
    /// assert_eq!(&n, Node::null_id());
    /// ```
    fn read_node(&mut self) -> io::Result<Node>;
}

impl<R: Read + ?Sized> ReadNodeExt for R {
    fn read_node(&mut self) -> io::Result<Node> {
        let mut node = Node([0u8; Node::len()]);
        self.read_exact(&mut node.0)?;
        Ok(node)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for Node {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let mut bytes = [0u8; Node::len()];
        g.fill_bytes(&mut bytes);
        Node::from(&bytes)
    }
}

#[cfg(any(test, feature = "for-tests"))]
pub mod mocks {
    use super::Node;

    pub const ONES: Node = Node([0x11; Node::len()]);
    pub const TWOS: Node = Node([0x22; Node::len()]);
    pub const THREES: Node = Node([0x33; Node::len()]);
    pub const FOURS: Node = Node([0x44; Node::len()]);
    pub const FIVES: Node = Node([0x55; Node::len()]);
    pub const SIXES: Node = Node([0x66; Node::len()]);
    pub const SEVENS: Node = Node([0x77; Node::len()]);
    pub const EIGHTS: Node = Node([0x88; Node::len()]);
    pub const NINES: Node = Node([0x99; Node::len()]);
    pub const AS: Node = Node([0xAA; Node::len()]);
    pub const BS: Node = Node([0xAB; Node::len()]);
    pub const CS: Node = Node([0xCC; Node::len()]);
    pub const DS: Node = Node([0xDD; Node::len()]);
    pub const ES: Node = Node([0xEE; Node::len()]);
    pub const FS: Node = Node([0xFF; Node::len()]);
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;

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
