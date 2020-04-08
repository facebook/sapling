/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This sub module contains functions to load hooks for the server

#![deny(warnings)]

use crate::errors::*;
use crate::facebook::rust_hooks::{
    always_fail_changeset::AlwaysFailChangeset, block_cross_repo_commits::BlockCrossRepoCommits,
    block_empty_commit::BlockEmptyCommit, check_nocommit::CheckNocommitHook,
    check_unittests::CheckUnittestsHook, conflict_markers::ConflictMarkers, deny_files::DenyFiles,
    ensure_valid_email::EnsureValidEmailHook,
    gitattributes_textdirectives::GitattributesTextDirectives,
    limit_commit_message_length::LimitCommitMessageLength, limit_commitsize::LimitCommitsize,
    limit_filesize::LimitFilesize, limit_path_length::LimitPathLengthHook,
    no_bad_filenames::NoBadFilenames, no_insecure_filenames::NoInsecureFilenames,
    no_questionable_filenames::NoQuestionableFilenames, signed_source::SignedSourceHook,
    tp2_symlinks_only::TP2SymlinksOnly, verify_integrity::VerifyIntegrityHook,
    verify_reviewedby_info::VerifyReviewedbyInfo,
};
use crate::{ChangesetHook, FileHook, Hook, HookChangeset, HookFile, HookManager};
use anyhow::Error;
use fbinit::FacebookInit;
use metaconfig_types::RepoConfig;
use std::collections::HashSet;
use std::sync::Arc;

enum LoadedRustHook {
    ChangesetHook(Arc<dyn Hook<HookChangeset>>),
    FileHook(Arc<dyn Hook<HookFile>>),
    #[allow(dead_code)]
    BonsaiChangesetHook(Box<dyn ChangesetHook>),
    #[allow(dead_code)]
    BonsaiFileHook(Box<dyn FileHook>),
}

pub fn load_hooks(
    fb: FacebookInit,
    hook_manager: &mut HookManager,
    config: RepoConfig,
    disabled_hooks: &HashSet<String>,
) -> Result<(), Error> {
    let mut hooks_not_disabled = disabled_hooks.clone();

    let mut hook_set = HashSet::new();
    for hook in config.hooks {
        use LoadedRustHook::*;
        let name = hook.name;

        if disabled_hooks.contains(&name) {
            hooks_not_disabled.remove(&name);
            continue;
        }

        // Backwards compatibility only
        let hook_name = if name.starts_with("rust:") {
            name[5..].to_string()
        } else {
            name.clone()
        };

        let rust_hook = match hook_name.as_ref() {
            "always_fail_changeset" => ChangesetHook(Arc::new(AlwaysFailChangeset::new())),
            "block_cross_repo_commits" => FileHook(Arc::new(BlockCrossRepoCommits::new())),
            "block_empty_commit" => ChangesetHook(Arc::new(BlockEmptyCommit::new())),
            "check_nocommit" => FileHook(Arc::new(CheckNocommitHook::new(&hook.config)?)),
            "check_unittests" => ChangesetHook(Arc::new(CheckUnittestsHook::new(&hook.config)?)),
            "conflict_markers" => FileHook(Arc::new(ConflictMarkers::new())),
            "deny_files" => FileHook(Arc::new(DenyFiles::new()?)),
            "ensure_valid_email" => {
                ChangesetHook(Arc::new(EnsureValidEmailHook::new(fb, &hook.config)?))
            }
            "gitattributes-textdirectives" => {
                FileHook(Arc::new(GitattributesTextDirectives::new()?))
            }
            "limit_commit_message_length" => {
                ChangesetHook(Arc::new(LimitCommitMessageLength::new(&hook.config)?))
            }
            "limit_commitsize" => ChangesetHook(Arc::new(LimitCommitsize::new(&hook.config))),
            "limit_filesize" => FileHook(Arc::new(LimitFilesize::new(&hook.config))),
            "limit_path_length" => FileHook(Arc::new(LimitPathLengthHook::new(&hook.config)?)),
            "no_bad_filenames" => FileHook(Arc::new(NoBadFilenames::new()?)),
            "no_insecure_filenames" => FileHook(Arc::new(NoInsecureFilenames::new()?)),
            "no_questionable_filenames" => FileHook(Arc::new(NoQuestionableFilenames::new()?)),
            "signed_source" => FileHook(Arc::new(SignedSourceHook::new(&hook.config)?)),
            "tp2_symlinks_only" => FileHook(Arc::new(TP2SymlinksOnly::new())),
            "verify_integrity" => ChangesetHook(Arc::new(VerifyIntegrityHook::new(&hook.config)?)),
            "verify_reviewedby_info" => {
                ChangesetHook(Arc::new(VerifyReviewedbyInfo::new(&hook.config)?))
            }
            _ => return Err(ErrorKind::InvalidRustHook(name.clone()).into()),
        };

        match rust_hook {
            FileHook(rust_hook) => hook_manager.register_file_hook(&name, rust_hook, hook.config),
            ChangesetHook(rust_hook) => {
                hook_manager.register_changeset_hook(&name, rust_hook, hook.config)
            }
            BonsaiFileHook(rust_hook) => {
                hook_manager.register_file_hook_new(&name, rust_hook, hook.config)
            }
            BonsaiChangesetHook(rust_hook) => {
                hook_manager.register_changeset_hook_new(&name, rust_hook, hook.config)
            }
        }

        hook_set.insert(name);
    }

    if hooks_not_disabled.len() > 0 {
        return Err(ErrorKind::NoSuchHookToDisable(hooks_not_disabled).into());
    }

    for bookmark_hook in config.bookmarks {
        let bookmark = bookmark_hook.bookmark;
        let hooks: Vec<_> = bookmark_hook
            .hooks
            .into_iter()
            .filter(|h| !disabled_hooks.contains(h))
            .collect();
        let bm_hook_set: HashSet<String> = hooks.clone().into_iter().collect();
        let diff: HashSet<_> = bm_hook_set.difference(&hook_set).collect();
        if diff.len() != 0 {
            return Err(ErrorKind::NoSuchBookmarkHook(
                bookmark,
                diff.into_iter().cloned().collect(),
            )
            .into());
        } else {
            hook_manager.set_hooks_for_bookmark(bookmark, hooks);
        }
    }

    Ok(())
}
