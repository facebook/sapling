/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! For Facebook hooks check the src/facebook/ folder

mod always_fail_changeset;
mod block_empty_commit;
mod check_nocommit;
mod conflict_markers;
pub(crate) mod deny_files;
mod limit_commit_message_length;
pub(crate) mod limit_commitsize;
pub(crate) mod limit_filesize;
mod limit_path_length;
mod lua_pattern;
pub(crate) mod no_bad_filenames;
mod no_insecure_filenames;
pub(crate) mod no_questionable_filenames;

use anyhow::Result;
use fbinit::FacebookInit;
use metaconfig_types::HookConfig;
use permission_checker::ArcMembershipChecker;

pub(crate) use self::lua_pattern::LuaPattern;
use crate::{ChangesetHook, FileHook};

pub fn hook_name_to_changeset_hook(
    _fb: FacebookInit,
    name: &str,
    config: &HookConfig,
    _reviewers_membership: ArcMembershipChecker,
) -> Result<Option<Box<dyn ChangesetHook>>> {
    Ok(match name {
        "always_fail_changeset" => {
            Some(Box::new(always_fail_changeset::AlwaysFailChangeset::new()))
        }
        "block_empty_commit" => Some(Box::new(block_empty_commit::BlockEmptyCommit::new())),
        "limit_commit_message_length" => Some(Box::new(
            limit_commit_message_length::LimitCommitMessageLength::new(config)?,
        )),
        "limit_commitsize" => Some(Box::new(
            limit_commitsize::LimitCommitsize::builder()
                .set_from_config(config)
                .build()?,
        )),
        _ => None,
    })
}

pub fn hook_name_to_file_hook(
    name: &str,
    config: &HookConfig,
) -> Result<Option<Box<dyn FileHook>>> {
    Ok(match name {
        "check_nocommit" => Some(Box::new(check_nocommit::CheckNocommitHook::new(config)?)),
        "conflict_markers" => Some(Box::new(conflict_markers::ConflictMarkers::new())),
        "deny_files" => Some(Box::new(
            deny_files::DenyFiles::builder()
                .set_from_config(config)
                .build()?,
        )),
        "limit_filesize" => Some(Box::new(
            limit_filesize::LimitFilesize::builder()
                .set_from_config(config)
                .build()?,
        )),
        "limit_path_length" => Some(Box::new(limit_path_length::LimitPathLengthHook::new(
            config,
        )?)),
        "no_bad_filenames" => Some(Box::new(
            no_bad_filenames::NoBadFilenames::builder()
                .set_from_config(config)
                .build()?,
        )),
        "no_insecure_filenames" => {
            Some(Box::new(no_insecure_filenames::NoInsecureFilenames::new()?))
        }
        "no_questionable_filenames" => Some(Box::new(
            no_questionable_filenames::NoQuestionableFilenames::builder()
                .set_from_config(config)
                .build()?,
        )),
        _ => None,
    })
}
