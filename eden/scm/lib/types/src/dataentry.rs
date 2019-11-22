/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use bytes::Bytes;
use crypto::{digest::Digest, sha1::Sha1};
use serde_derive::{Deserialize, Serialize};

use crate::{hgid::HgId, key::Key, parents::Parents};

/// Tombstone string to replace the content of blacklisted files with
/// TODO(T48685378): Handle redacted content in a less hacky way
const REDACTED_TOMBSTONE: &str =
    "PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oHAF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNoUI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQV3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n";

/// Structure representing source control data (typically
/// either file content or a tree entry) on the wire.
/// Includes the information required to add the data to
/// a MutableDataPack, along with the hgid's parent
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
    key: Key,
    data: Bytes,
    parents: Parents,
}

/// Enum representing the results of attempting to validate a DataEntry
/// by computing the expected filenode hash of its content. Due to various
/// corner cases, the result of such a validation is more complex than
/// a simple boolean.
pub enum Validity {
    /// Filenode hash successfully validated.
    Valid,
    /// Data entry was redacted by the server. The received content
    /// did not validate but matches the known tombstone content for
    /// redacted data.
    Redacted,
    /// Validation failed, but the path associated with this data is
    /// empty. If this DataEntry represents a tree manifest hgid, this
    /// situation is sometimes expected in legacy situations involving
    /// hybrid tree manifests. The filenode hash represents is that of
    /// a flat manifest while the data is the content of a root tree
    /// manifest. Given that this situation does occur in practice,
    /// this is a separate variant that higher-level code can choose
    /// to treat as a special case.
    InvalidEmptyPath(Error),
    /// Validation failed.
    Invalid(Error),
}

impl DataEntry {
    pub fn new(key: Key, data: Bytes, parents: Parents) -> Self {
        Self { key, data, parents }
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    /// Get this entry's data content. This method checks the validity of the
    /// data and return the validation result along with the data iself.
    pub fn data(&self) -> (Bytes, Validity) {
        (self.data.clone(), self.validate())
    }

    /// Compute the filenode hash of this `DataEntry` using its parents and
    /// content, and compare it with the known hgid hash from the entry's `Key`.
    fn validate(&self) -> Validity {
        // TODO(T48685378): Handle redacted content in a less hacky ways
        if self.data.len() == REDACTED_TOMBSTONE.len() && self.data == REDACTED_TOMBSTONE {
            return Validity::Redacted;
        }

        // Mercurial hashes the parent nodes in sorted order
        // when computing the hgid hash.
        let (p1, p2) = match self.parents.clone().into_nodes() {
            (p1, p2) if p1 > p2 => (p2, p1),
            (p1, p2) => (p1, p2),
        };

        let mut hash = [0u8; 20];
        let mut hasher = Sha1::new();
        hasher.input(p1.as_ref());
        hasher.input(p2.as_ref());
        hasher.input(&self.data);
        hasher.result(&mut hash);

        let computed = HgId::from_byte_array(hash);
        let expected = &self.key.hgid;

        if &computed != expected {
            let err = format_err!(
                "Content hash validation failed. Expected: {}; Computed: {}; Parents: (p1: {}, p2: {})",
                expected.to_hex(),
                computed.to_hex(),
                p1.to_hex(),
                p2.to_hex(),
            );
            if self.key.path.is_empty() {
                return Validity::InvalidEmptyPath(err);
            } else {
                return Validity::Invalid(err);
            }
        }

        Validity::Valid
    }
}
