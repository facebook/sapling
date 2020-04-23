/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! For Facebook hooks check the src/facebook/ folder

use anyhow::Result;
use fbinit::FacebookInit;
use metaconfig_types::HookConfig;
use permission_checker::ArcMembershipChecker;

use crate::{ChangesetHook, FileHook};

pub fn hook_name_to_changeset_hook(
    _fb: FacebookInit,
    _name: &str,
    _config: &HookConfig,
    _reviewers_membership: ArcMembershipChecker,
) -> Result<Option<Box<dyn ChangesetHook>>> {
    Ok(None)
}

pub fn hook_name_to_file_hook(
    _name: &str,
    _config: &HookConfig,
) -> Result<Option<Box<dyn FileHook>>> {
    Ok(None)
}
