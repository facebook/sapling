/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::ArgEnum;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use strum::IntoEnumIterator;
use strum_macros::AsRefStr;
use strum_macros::EnumString;
use strum_macros::EnumVariantNames;

use crate::detail::graph::NodeType;
use crate::detail::state::InternedType;

#[derive(Debug, Clone, Copy, ArgEnum, AsRefStr, EnumString, EnumVariantNames)]
pub enum InternedTypeArg {
    All,
    FileUnodeId,
    HgChangesetId,
    HgFileNodeId,
    HgManifestId,
    ManifestUnodeId,
    MPathHash,
}

impl InternedTypeArg {
    pub fn parse_args(args: &[Self]) -> HashSet<InternedType> {
        let mut int_types = HashSet::new();
        for arg in args {
            match *arg {
                InternedTypeArg::FileUnodeId => {
                    int_types.insert(InternedType::FileUnodeId);
                }
                InternedTypeArg::HgChangesetId => {
                    int_types.insert(InternedType::HgChangesetId);
                }
                InternedTypeArg::HgFileNodeId => {
                    int_types.insert(InternedType::HgFileNodeId);
                }
                InternedTypeArg::HgManifestId => {
                    int_types.insert(InternedType::HgManifestId);
                }
                InternedTypeArg::ManifestUnodeId => {
                    int_types.insert(InternedType::ManifestUnodeId);
                }
                InternedTypeArg::MPathHash => {
                    int_types.insert(InternedType::MPathHash);
                }
                InternedTypeArg::All => {
                    int_types.extend(InternedType::iter());
                }
            }
        }
        int_types
    }
}

/// Default to clearing out all except HgChangesets
pub const DEFAULT_INTERNED_TYPES: &[InternedTypeArg] = &[
    InternedTypeArg::FileUnodeId,
    InternedTypeArg::HgFileNodeId,
    InternedTypeArg::HgManifestId,
    InternedTypeArg::ManifestUnodeId,
    InternedTypeArg::MPathHash,
];

// clap doesn't allow to pass typed default values for some reason, let's convert them
pub static DEFAULT_INTERNED_TYPES_STR: Lazy<Vec<&'static str>> = Lazy::new(|| {
    DEFAULT_INTERNED_TYPES
        .iter()
        .map(|int_type| int_type.as_ref())
        .collect()
});

/// We can jump for ChangesetId to all of these
#[derive(Debug, Clone, Copy, ArgEnum, AsRefStr, EnumString, EnumVariantNames)]
pub enum ChunkByPublicArg {
    BonsaiHgMapping,
    PhaseMapping,
    Changeset,
    ChangesetInfo,
    ChangesetInfoMapping,
    DeletedManifestV2Mapping,
    FsnodeMapping,
    SkeletonManifestMapping,
    UnodeMapping,
}

impl From<ChunkByPublicArg> for NodeType {
    fn from(arg: ChunkByPublicArg) -> NodeType {
        match arg {
            ChunkByPublicArg::BonsaiHgMapping => NodeType::BonsaiHgMapping,
            ChunkByPublicArg::PhaseMapping => NodeType::PhaseMapping,
            ChunkByPublicArg::Changeset => NodeType::Changeset,
            ChunkByPublicArg::ChangesetInfo => NodeType::ChangesetInfo,
            ChunkByPublicArg::ChangesetInfoMapping => NodeType::ChangesetInfoMapping,
            ChunkByPublicArg::DeletedManifestV2Mapping => NodeType::DeletedManifestV2Mapping,
            ChunkByPublicArg::FsnodeMapping => NodeType::FsnodeMapping,
            ChunkByPublicArg::SkeletonManifestMapping => NodeType::SkeletonManifestMapping,
            ChunkByPublicArg::UnodeMapping => NodeType::UnodeMapping,
        }
    }
}

impl ChunkByPublicArg {
    pub fn parse_args(args: &[Self]) -> HashSet<NodeType> {
        args.iter().cloned().map(NodeType::from).collect()
    }
}
