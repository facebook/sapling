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
use strum_macros::{AsRefStr, EnumString, EnumVariantNames};

use walker_commands_impl::state::InternedType;

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
