/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! For Facebook hooks check the src/facebook/ folder

mod always_fail_changeset;
mod block_commit_message_pattern;
mod block_content_pattern;
mod block_empty_commit;
mod block_files;
pub(crate) mod deny_files;
mod limit_commit_message_length;
pub(crate) mod limit_commit_size;
pub(crate) mod limit_filesize;
mod limit_path_length;
pub(crate) mod no_bad_extensions;
pub(crate) mod no_bad_filenames;
mod no_insecure_filenames;
pub(crate) mod no_questionable_filenames;
pub(crate) mod no_windows_filenames;
pub(crate) mod require_commit_message_pattern;

use anyhow::Result;
use fbinit::FacebookInit;
use metaconfig_types::HookParams;
use permission_checker::AclProvider;
use permission_checker::ArcMembershipChecker;

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
        "block_commit_message_pattern" => Some(b(
            block_commit_message_pattern::BlockCommitMessagePatternHook::new(&params.config)?,
        )),
        "block_empty_commit" => Some(b(block_empty_commit::BlockEmptyCommit::new())),
        "limit_commit_message_length" => {
            let hook =
                limit_commit_message_length::LimitCommitMessageLengthHook::new(&params.config)?;
            Some(b(hook))
        }

        "limit_commit_size" => Some(b(limit_commit_size::LimitCommitSizeHook::new(
            &params.config,
        )?)),
        "require_commit_message_pattern" => Some(b(
            require_commit_message_pattern::RequireCommitMessagePatternHook::new(&params.config)?,
        )),
        _ => None,
    })
}

pub fn make_file_hook(
    _fb: FacebookInit,
    params: &HookParams,
) -> Result<Option<Box<dyn FileHook + 'static>>> {
    Ok(match params.implementation.as_str() {
        "block_content_pattern" => Some(Box::new(
            block_content_pattern::BlockContentPatternHook::new(&params.config)?,
        )),
        "block_files" => Some(Box::new(block_files::BlockFilesHook::new(&params.config)?)),
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
        "limit_path_length" => {
            let hook = limit_path_length::LimitPathLengthHook::new(&params.config)?;
            Some(Box::new(hook))
        }
        "no_bad_filenames" => Some(Box::new(no_bad_filenames::NoBadFilenamesHook::new(
            &params.config,
        )?)),
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
            no_windows_filenames::NoWindowsFilenamesHook::new(&params.config)?,
        )),
        _ => None,
    })
}
