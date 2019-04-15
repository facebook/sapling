// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

use serde_derive::{Deserialize, Serialize};

use crate::node::Node;

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
pub struct Key {
    // Name is usually a file or directory path
    pub(crate) name: Vec<u8>,
    // Node is always a 20 byte hash. This will be changed to a fix length array later.
    pub node: Node,
}

impl Key {
    pub fn new(name: Vec<u8>, node: Node) -> Self {
        Key { name, node }
    }

    pub fn name(&self) -> &[u8] {
        &self.name
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", &self.node, String::from_utf8_lossy(&self.name))
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for Key {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Key::new(Vec::arbitrary(g), Node::arbitrary(g))
    }
}

#[cfg(any(test, feature = "for-tests"))]
pub mod mocks {
    use super::Key;
    use crate::node::mocks::{ONES, THREES, TWOS};

    use lazy_static::lazy_static;

    lazy_static! {
        pub static ref FOO_KEY: Key = Key::new(b"foo".to_vec(), ONES);
        pub static ref BAR_KEY: Key = Key::new(b"bar".to_vec(), TWOS);
        pub static ref BAZ_KEY: Key = Key::new(b"baz".to_vec(), THREES);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use mocks::*;

    #[test]
    fn display_key() {
        let foo = "1111111111111111111111111111111111111111 foo";
        let bar = "2222222222222222222222222222222222222222 bar";
        let baz = "3333333333333333333333333333333333333333 baz";
        assert_eq!(format!("{}", &*FOO_KEY), foo);
        assert_eq!(format!("{}", &*BAR_KEY), bar);
        assert_eq!(format!("{}", &*BAZ_KEY), baz);
    }
}
