/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::detail::validate::CheckType;
use crate::detail::validate::DEFAULT_CHECK_TYPES;
use clap::ArgEnum;
use clap::Args;
use std::collections::HashSet;
use strum_macros::AsRefStr;
use strum_macros::EnumString;
use strum_macros::EnumVariantNames;

#[derive(Args, Debug)]
pub struct ValidateCheckTypeArgs {
    /// Check types to exclude
    #[clap(long, short = 'C')]
    pub exclude_check_type: Vec<CheckTypeArg>,
    /// Check types to include
    // TODO(aida): After the D26853691 it doesn't actually default to all
    // possible values, `CheckType::FileContentIsLfs` is missing.
    // We need to fix this (then we can also get rid of DEFAULT_CHECK_TYPES).
    #[clap(long, short = 'c', default_values = &[CheckTypeArg::All.as_ref()])]
    pub include_check_type: Vec<CheckTypeArg>,
}

impl ValidateCheckTypeArgs {
    pub fn parse_args(&self) -> HashSet<CheckType> {
        let mut include_types = parse_check_type_args(&self.include_check_type);
        let exclude_types = parse_check_type_args(&self.exclude_check_type);
        include_types.retain(|x| !exclude_types.contains(x));
        include_types
    }
}

#[derive(Debug, Clone, Copy, ArgEnum, AsRefStr, EnumString, EnumVariantNames)]
pub enum CheckTypeArg {
    All,
    ChangesetPhaseIsPublic,
    HgLinkNodePopulated,
    FileContentIsLfs,
}

fn parse_check_type_args(check_type_args: &[CheckTypeArg]) -> HashSet<CheckType> {
    let mut check_types = HashSet::new();
    for arg in check_type_args {
        match arg {
            CheckTypeArg::All => {
                for default_type in DEFAULT_CHECK_TYPES {
                    check_types.insert(default_type.clone());
                }
            }
            CheckTypeArg::ChangesetPhaseIsPublic => {
                check_types.insert(CheckType::ChangesetPhaseIsPublic);
            }
            CheckTypeArg::HgLinkNodePopulated => {
                check_types.insert(CheckType::HgLinkNodePopulated);
            }
            CheckTypeArg::FileContentIsLfs => {
                check_types.insert(CheckType::FileContentIsLfs);
            }
        }
    }
    check_types
}
