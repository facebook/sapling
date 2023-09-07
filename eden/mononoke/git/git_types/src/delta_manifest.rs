/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use fbthrift::compact_protocol;
use gix_hash::oid;
use gix_hash::ObjectId;
use mononoke_types::path::MPath;
use mononoke_types::thrift as mononoke_types_thrift;
use mononoke_types::ChangesetId;
use mononoke_types::ThriftConvert;
use quickcheck::Arbitrary;

use crate::thrift;

/// Represents a single entry in the GitDeltaManifest corresponding to a Git object.
/// Contains reference to the full version of the object along with all potential delta entries.
/// The delta variants would be absent if the object is introduced for the first time or if the
/// object is too large (or of unsupported type) to be represented as a delta
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GitDeltaManifestEntry {
    /// The full version of the Git object
    full: ObjectEntry,
    /// The delta variant of the Git object against all possible base objects
    deltas: Vec<ObjectDelta>,
}

impl GitDeltaManifestEntry {
    pub fn new(full: ObjectEntry, deltas: Vec<ObjectDelta>) -> Self {
        Self { full, deltas }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let thrift_tc: thrift::GitDeltaManifestEntry = compact_protocol::deserialize(bytes)
            .context("Failure in deserializing bytes to GitDeltaManifestEntry")?;
        thrift_tc
            .try_into()
            .context("Failure in converting Thrift data to GitDeltaManifestEntry")
    }
}

impl TryFrom<thrift::GitDeltaManifestEntry> for GitDeltaManifestEntry {
    type Error = Error;

    fn try_from(value: thrift::GitDeltaManifestEntry) -> Result<Self, Self::Error> {
        let full = value.full.try_into()?;
        let deltas = value
            .deltas
            .into_iter()
            .map(|d| d.try_into())
            .collect::<Result<Vec<_>>>()?;
        Ok(GitDeltaManifestEntry { full, deltas })
    }
}

impl From<GitDeltaManifestEntry> for thrift::GitDeltaManifestEntry {
    fn from(value: GitDeltaManifestEntry) -> Self {
        let full = value.full.into();
        let deltas = value.deltas.into_iter().map(|d| d.into()).collect();
        thrift::GitDeltaManifestEntry { full, deltas }
    }
}

impl ThriftConvert for GitDeltaManifestEntry {
    const NAME: &'static str = "GitDeltaManifestEntry";
    type Thrift = thrift::GitDeltaManifestEntry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

impl Arbitrary for GitDeltaManifestEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let full = ObjectEntry::arbitrary(g);
        let deltas = Vec::arbitrary(g);
        GitDeltaManifestEntry { full, deltas }
    }
}

/// Represents the delta for a single Git object
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ObjectDelta {
    /// The commit that originally introduced this Git object
    origin: ChangesetId,
    /// The base Git object used for creating the delta
    base: ObjectEntry,
    /// Raw Zlib encoded instructions for recreating this object from the base
    encoded_instructions: Bytes,
}

impl TryFrom<thrift::ObjectDelta> for ObjectDelta {
    type Error = Error;

    fn try_from(value: thrift::ObjectDelta) -> Result<Self, Self::Error> {
        let base = value.base.try_into()?;
        let encoded_instructions = value.encoded_instructions;
        let origin = ChangesetId::from_thrift(value.origin)?;
        Ok(Self {
            base,
            origin,
            encoded_instructions,
        })
    }
}

impl From<ObjectDelta> for thrift::ObjectDelta {
    fn from(value: ObjectDelta) -> Self {
        let base = value.base.into();
        let encoded_instructions = value.encoded_instructions;
        let origin = ChangesetId::into_thrift(value.origin);
        Self {
            base,
            origin,
            encoded_instructions,
        }
    }
}

impl ThriftConvert for ObjectDelta {
    const NAME: &'static str = "ObjectDelta";
    type Thrift = thrift::ObjectDelta;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

impl Arbitrary for ObjectDelta {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let base = ObjectEntry::arbitrary(g);
        let origin = ChangesetId::arbitrary(g);
        let encoded_instructions = Bytes::from(Vec::arbitrary(g));
        Self {
            base,
            origin,
            encoded_instructions,
        }
    }
}

/// Metadata information representing a Git object to be used in
/// the GitDeltaManifest
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ObjectEntry {
    /// The Git object ID which is the SHA1 hash of the object content
    oid: ObjectId,
    /// The size of the object in bytes
    size: u64,
    /// The type of the Git object, only Blob and Tree are supported in GitDeltaManifest
    kind: ObjectKind,
    /// The path of the directory or file corresponding to this Git Tree or Blob
    path: MPath,
}

impl TryFrom<thrift::ObjectEntry> for ObjectEntry {
    type Error = Error;

    fn try_from(t: thrift::ObjectEntry) -> Result<Self, Error> {
        let oid = oid::try_from_bytes(&t.oid.0)?.to_owned();
        let size: u64 = t.size.try_into()?;
        let kind = t.kind.try_into()?;
        let path = MPath::from_thrift(t.path)?;
        Ok(Self {
            oid,
            size,
            kind,
            path,
        })
    }
}

impl From<ObjectEntry> for thrift::ObjectEntry {
    fn from(value: ObjectEntry) -> Self {
        let oid = mononoke_types_thrift::GitSha1(value.oid.as_bytes().into());
        let size = value.size as i64;
        let kind = value.kind.into();
        let path = MPath::into_thrift(value.path);
        thrift::ObjectEntry {
            oid,
            size,
            kind,
            path,
        }
    }
}

impl ThriftConvert for ObjectEntry {
    const NAME: &'static str = "ObjectEntry";
    type Thrift = thrift::ObjectEntry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

impl Arbitrary for ObjectEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let oid = oid::try_from_bytes(mononoke_types::hash::Sha1::arbitrary(g).as_ref())
            .unwrap()
            .into();
        let size = u64::arbitrary(g) / 2;
        let kind = ObjectKind::arbitrary(g);
        let path = MPath::arbitrary(g);
        Self {
            oid,
            size,
            kind,
            path,
        }
    }
}

/// Enum representing the types of Git objects that can be present
/// in a GitDeltaManifest
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ObjectKind {
    Blob,
    Tree,
}

impl TryFrom<thrift::ObjectKind> for ObjectKind {
    type Error = Error;

    fn try_from(value: thrift::ObjectKind) -> Result<Self, Self::Error> {
        match value {
            thrift::ObjectKind::Blob => Ok(Self::Blob),
            thrift::ObjectKind::Tree => Ok(Self::Tree),
            thrift::ObjectKind(x) => anyhow::bail!("Unsupported object kind: {}", x),
        }
    }
}

impl From<ObjectKind> for thrift::ObjectKind {
    fn from(value: ObjectKind) -> Self {
        match value {
            ObjectKind::Blob => thrift::ObjectKind::Blob,
            ObjectKind::Tree => thrift::ObjectKind::Tree,
        }
    }
}

impl ThriftConvert for ObjectKind {
    const NAME: &'static str = "ObjectKind";
    type Thrift = thrift::ObjectKind;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        t.try_into()
    }

    fn into_thrift(self) -> Self::Thrift {
        self.into()
    }
}

impl Arbitrary for ObjectKind {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        match bool::arbitrary(g) {
            true => ObjectKind::Blob,
            false => ObjectKind::Tree,
        }
    }
}

#[cfg(test)]
mod test {
    use quickcheck::quickcheck;

    use super::*;

    quickcheck! {
        fn git_delta_manifest_entry_thrift_roundtrip(entry: GitDeltaManifestEntry) -> bool {
            let thrift_entry: thrift::GitDeltaManifestEntry = entry.clone().into();
            let from_thrift_entry: GitDeltaManifestEntry = thrift_entry.try_into().expect("thrift roundtrips should always be valid");
            println!("entry: {:?}", entry);
            println!("entry_from_thrift: {:?}", from_thrift_entry);
            entry == from_thrift_entry
        }
    }
}
