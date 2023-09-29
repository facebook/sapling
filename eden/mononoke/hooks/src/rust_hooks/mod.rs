/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
pub(crate) mod no_bad_extensions;
pub(crate) mod no_bad_filenames;
mod no_insecure_filenames;
pub(crate) mod no_questionable_filenames;
pub(crate) mod no_windows_filenames;

use anyhow::Result;
use fbinit::FacebookInit;
use metaconfig_types::HookParams;
use permission_checker::AclProvider;
use permission_checker::ArcMembershipChecker;

pub(crate) use self::lua_pattern::LuaPattern;
use crate::ChangesetHook;
use crate::FileHook;

fn b(t: impl ChangesetHook + 'static) -> Box<dyn ChangesetHook> {
    Box::new(t)
}

pub async fn make_changeset_hook(
    _fb: FacebookInit,
    params: &HookParams,
    _acl_provider: &dyn AclProvider,
    _reviewers_membership: ArcMembershipChecker,
    _repo_name: &str,
) -> Result<Option<Box<dyn ChangesetHook + 'static>>> {
    Ok(match params.implementation.as_str() {
        "always_fail_changeset" => Some(b(always_fail_changeset::AlwaysFailChangeset::new())),
        "block_empty_commit" => Some(b(block_empty_commit::BlockEmptyCommit::new())),
        "check_nocommit_message" => {
            Some(b(check_nocommit::CheckNocommitHook::new(&params.config)?))
        }
        "limit_commit_message_length" => Some(b(
            limit_commit_message_length::LimitCommitMessageLength::new(&params.config)?,
        )),
        "limit_commitsize" => Some(b(limit_commitsize::LimitCommitsize::builder()
            .set_from_config(&params.config)
            .build()?)),
        _ => None,
    })
}

pub fn make_file_hook(
    _fb: FacebookInit,
    params: &HookParams,
) -> Result<Option<Box<dyn FileHook + 'static>>> {
    Ok(match params.implementation.as_str() {
        "check_nocommit" => Some(Box::new(check_nocommit::CheckNocommitHook::new(
            &params.config,
        )?)),
        "conflict_markers" => Some(Box::new(conflict_markers::ConflictMarkers::new())),
        "deny_files" => Some(Box::new(
            deny_files::DenyFiles::builder()
                .set_from_config(&params.config)
                .build()?,
        )),
        "limit_filesize" => Some(Box::new(
            limit_filesize::LimitFilesize::builder()
                .set_from_config(&params.config)
                .build()?,
        )),
        "limit_path_length" => Some(Box::new(limit_path_length::LimitPathLengthHook::new(
            &params.config,
        )?)),
        "no_bad_filenames" => Some(Box::new(
            no_bad_filenames::NoBadFilenames::builder()
                .set_from_config(&params.config)
                .build()?,
        )),
        "no_bad_extensions" => Some(Box::new(
            no_bad_extensions::NoBadExtensions::builder()
                .set_from_config(&params.config)
                .build()?,
        )),
        "no_insecure_filenames" => {
            Some(Box::new(no_insecure_filenames::NoInsecureFilenames::new()?))
        }
        "no_questionable_filenames" => Some(Box::new(
            no_questionable_filenames::NoQuestionableFilenames::builder()
                .set_from_config(&params.config)
                .build()?,
        )),
        "no_windows_filenames" => Some(Box::new(
            no_windows_filenames::NoWindowsFilenames::builder()
                .set_from_config(&params.config)
                .build()?,
        )),
        _ => None,
    })
}
