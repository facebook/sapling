// Copyright Facebook, Inc. 2019.

use bytes::Bytes;
use crypto::{digest::Digest, sha1::Sha1};
use failure::{ensure, Fallible};
use serde_derive::{Deserialize, Serialize};

use crate::{key::Key, node::Node, parents::Parents};

/// Structure representing a file's data content on the wire.
/// Includes the information required to add the file to
/// a MutableDataPack, along with the filenode parent
/// information to allow for hash verification.
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
pub struct DataEntry {
    pub key: Key,
    pub data: Bytes,
    pub parents: Parents,
}

impl DataEntry {
    /// Compute the filenode hash of this `DataEntry` using the parents and
    /// file content, and compare it with the known filenode hash from
    /// the entry's `Key`.
    pub fn validate(&self) -> Fallible<()> {
        let (p1, p2) = self.parents.clone().into_nodes();
        let mut hash = [0u8; 20];
        let mut hasher = Sha1::new();

        hasher.input(p1.as_ref());
        hasher.input(p2.as_ref());
        hasher.input(&self.data);
        hasher.result(&mut hash);

        let computed = Node::from_byte_array(hash);
        let expected = &self.key.node;

        ensure!(
            &computed == expected,
            "Filenode validation failed. Expected: {}; Computed: {}",
            expected.to_hex(),
            computed.to_hex()
        );

        Ok(())
    }
}
