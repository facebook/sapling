/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use strum::AsRefStr;
use strum::Display;
use strum::EnumIter;
use strum::EnumString;

use crate::thrift;

/// Enum which consolidates all available derived data types
/// It provides access to `const &'static str` representation to
/// use as Name of the derived data type, which is used to
/// identify or name data (for example lease keys) associated with this
/// particular derived data type.
/// It also provides `as_ref()` method for serialization.
/// and implements FromStr trait for deserialization.
#[derive(
    AsRefStr, EnumString, Display, Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, Ord,
    PartialOrd
)]
pub enum DerivableType {
    BlameV2,
    BssmV3,
    ChangesetInfo,
    DeletedManifests,
    Fastlog,
    FileNodes,
    Fsnodes,
    HgChangesets,
    GitTrees,
    GitCommits,
    GitDeltaManifests,
    SkeletonManifests,
    TestManifests,
    TestShardedManifests,
    Unodes,
}

impl DerivableType {
    pub fn from_name(s: &str) -> Result<Self> {
        // We need the duplication here to make it a `const fn` so it can be used in
        // BonsaiDerivable::NAME
        Ok(match s {
            "blame" => DerivableType::BlameV2,
            "bssm_v3" => DerivableType::BssmV3,
            "changeset_info" => DerivableType::ChangesetInfo,
            "deleted_manifest" => DerivableType::DeletedManifests,
            "fastlog" => DerivableType::Fastlog,
            "filenodes" => DerivableType::FileNodes,
            "fsnodes" => DerivableType::Fsnodes,
            "hgchangesets" => DerivableType::HgChangesets,
            "git_trees" => DerivableType::GitTrees,
            "git_commits" => DerivableType::GitCommits,
            "git_delta_manifests" => DerivableType::GitDeltaManifests,
            "skeleton_manifests" => DerivableType::SkeletonManifests,
            "test_manifests" => DerivableType::TestManifests,
            "test_sharded_manifests" => DerivableType::TestShardedManifests,
            "unodes" => DerivableType::Unodes,
            _ => bail!("invalid name for DerivedDataType: {}", s),
        })
    }
    pub const fn name(&self) -> &'static str {
        // We need the duplication here to make it a `const fn` so it can be used in
        // BonsaiDerivable::NAME
        match self {
            DerivableType::BlameV2 => "blame",
            DerivableType::BssmV3 => "bssm_v3",
            DerivableType::ChangesetInfo => "changeset_info",
            DerivableType::DeletedManifests => "deleted_manifest",
            DerivableType::Fastlog => "fastlog",
            DerivableType::FileNodes => "filenodes",
            DerivableType::Fsnodes => "fsnodes",
            DerivableType::HgChangesets => "hgchangesets",
            DerivableType::GitTrees => "git_trees",
            DerivableType::GitCommits => "git_commits",
            DerivableType::GitDeltaManifests => "git_delta_manifests",
            DerivableType::SkeletonManifests => "skeleton_manifests",
            DerivableType::TestManifests => "test_manifests",
            DerivableType::TestShardedManifests => "test_sharded_manifests",
            DerivableType::Unodes => "unodes",
        }
    }
    pub fn from_thrift(other: thrift::DerivedDataType) -> Result<Self> {
        Ok(match other {
            thrift::DerivedDataType::BLAME => Self::BlameV2,
            thrift::DerivedDataType::BSSM_V3 => Self::BssmV3,
            thrift::DerivedDataType::CHANGESET_INFO => Self::ChangesetInfo,
            thrift::DerivedDataType::DELETED_MANIFEST_V2 => Self::DeletedManifests,
            thrift::DerivedDataType::FASTLOG => Self::Fastlog,
            thrift::DerivedDataType::FILENODE => Self::FileNodes,
            thrift::DerivedDataType::FSNODE => Self::Fsnodes,
            thrift::DerivedDataType::HG_CHANGESET => Self::HgChangesets,
            thrift::DerivedDataType::TREE_HANDLE => Self::GitTrees,
            thrift::DerivedDataType::COMMIT_HANDLE => Self::GitCommits,
            thrift::DerivedDataType::GIT_DELTA_MANIFEST => Self::GitDeltaManifests,
            thrift::DerivedDataType::SKELETON_MANIFEST => Self::SkeletonManifests,
            thrift::DerivedDataType::TEST_MANIFEST => Self::TestManifests,
            thrift::DerivedDataType::TEST_SHARDED_MANIFEST => Self::TestShardedManifests,
            thrift::DerivedDataType::UNODE => Self::Unodes,
            _ => bail!("invalid thrift value for DerivedDataType: {:?}", other),
        })
    }
    pub fn into_thrift(&self) -> thrift::DerivedDataType {
        match self {
            Self::BlameV2 => thrift::DerivedDataType::BLAME,
            Self::BssmV3 => thrift::DerivedDataType::BSSM_V3,
            Self::ChangesetInfo => thrift::DerivedDataType::CHANGESET_INFO,
            Self::DeletedManifests => thrift::DerivedDataType::DELETED_MANIFEST_V2,
            Self::Fastlog => thrift::DerivedDataType::FASTLOG,
            Self::FileNodes => thrift::DerivedDataType::FILENODE,
            Self::Fsnodes => thrift::DerivedDataType::FSNODE,
            Self::HgChangesets => thrift::DerivedDataType::HG_CHANGESET,
            Self::GitTrees => thrift::DerivedDataType::TREE_HANDLE,
            Self::GitCommits => thrift::DerivedDataType::COMMIT_HANDLE,
            Self::GitDeltaManifests => thrift::DerivedDataType::GIT_DELTA_MANIFEST,
            Self::SkeletonManifests => thrift::DerivedDataType::SKELETON_MANIFEST,
            Self::TestManifests => thrift::DerivedDataType::TEST_MANIFEST,
            Self::TestShardedManifests => thrift::DerivedDataType::TEST_SHARDED_MANIFEST,
            Self::Unodes => thrift::DerivedDataType::UNODE,
            // If the compiler reminds you to add something here, please don't forget to also
            // update the `from_thrift` implementation above.
            // The unit test: `thrift_derived_data_type_conversion_must_be_bidirectional` in this
            // file should prevent you from forgetting at diff time.
        }
    }
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::DerivableType;

    #[test]
    fn thrift_derived_data_type_conversion_must_be_bidirectional() {
        for variant in DerivableType::iter() {
            assert_eq!(
                variant,
                DerivableType::from_thrift(variant.into_thrift())
                    .expect("Failed to convert back to DerivableType from thrift")
            );
        }
    }
    #[test]
    fn name_derived_data_type_conversion_must_be_bidirectional() {
        for variant in DerivableType::iter() {
            assert_eq!(
                variant,
                DerivableType::from_name(variant.name()).expect(
                    "Failed to convert back to DerivableType from its string representation with DerivableType::name"
                )
            );
        }
    }
}
