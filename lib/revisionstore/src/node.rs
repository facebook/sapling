// Copyright Facebook, Inc. 2018

/// A 20-byte identifier, often a hash. Nodes are used to uniquely identify
/// commits, file versions, and many other things.
pub struct Node([u8; 20]);

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
