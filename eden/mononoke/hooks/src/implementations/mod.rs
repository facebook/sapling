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
mod conflict_markers;
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

use anyhow::Result;
use fbinit::FacebookInit;
use metaconfig_types::HookParams;
use permission_checker::AclProvider;
use permission_checker::ArcMembershipChecker;
use regex::Regex;

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
        "check_nocommit_message" => {
            // Implement old hook behaviour directly during the transition.
            Some(Box::new(
                block_commit_message_pattern::BlockCommitMessagePatternHook::with_config(
                    block_commit_message_pattern::BlockCommitMessagePatternConfig {
                        pattern: Regex::new(
                            "(?i)(\x40(nocommit|no-commit|do-not-commit|do_not_commit))(\\W|_|\\z)",
                        )
                        .unwrap(),
                        message: String::from("Commit message contains a nocommit marker: ${1}"),
                    },
                )?,
            ))
        }
        "limit_commit_message_length" => Some(b(
            limit_commit_message_length::LimitCommitMessageLength::new(&params.config)?,
        )),
        "limit_commit_size" => Some(b(limit_commit_size::LimitCommitSizeHook::new(
            &params.config,
        )?)),
        // Implement old hook behaviour during the transisiton
        "limit_commitsize" => Some(b(limit_commit_size::legacy_limit_commitsize(
            &params.config,
        )?)),
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
        "check_nocommit" => {
            // Implement old hook behaviour directly during the transition.
            Some(Box::new(
                block_content_pattern::BlockContentPatternHook::with_config(
                    block_content_pattern::BlockContentPatternConfig {
                        pattern: Regex::new(
                            "(?i)(\x40(nocommit|no-commit|do-not-commit|do_not_commit))(\\W|_|\\z)",
                        )
                        .unwrap(),
                        message: String::from("File contains a ${1} marker"),
                    },
                )?,
            ))
        }
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
