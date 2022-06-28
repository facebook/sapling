/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use sql::mysql;

use mononoke_types::hash;
use mononoke_types::hash::Blake2;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
#[derive(mysql::OptTryFromRowField)]
pub struct IdMapVersion(pub u64);

impl fmt::Display for IdMapVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl IdMapVersion {
    pub fn bump(&self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(mysql::OptTryFromRowField)]
pub struct IdDagVersion(pub Blake2);

impl IdDagVersion {
    pub fn from_serialized_bytes<B: AsRef<[u8]>>(bytes: B) -> Self {
        let mut blake2_builder = hash::Context::new("iddag_version".as_bytes());
        blake2_builder.update(bytes);
        Self(blake2_builder.finish())
    }
}

impl fmt::Display for IdDagVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SegmentedChangelogVersion {
    pub iddag_version: IdDagVersion,
    pub idmap_version: IdMapVersion,
}

impl SegmentedChangelogVersion {
    pub fn new(iddag_version: IdDagVersion, idmap_version: IdMapVersion) -> Self {
        Self {
            iddag_version,
            idmap_version,
        }
    }
}

impl From<(IdDagVersion, IdMapVersion)> for SegmentedChangelogVersion {
    fn from(t: (IdDagVersion, IdMapVersion)) -> Self {
        SegmentedChangelogVersion::new(t.0, t.1)
    }
}
