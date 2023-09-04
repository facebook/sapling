/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use lazy_static::lazy_static;
use mononoke_types::BasicFileChange;
use mononoke_types::BonsaiChangeset;
use mononoke_types::NonRootMPath;
use regex::Regex;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileContentManager;
use crate::FileHook;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

const NOCOMMIT_REGEX: &str =
    "(?i)(\x40(nocommit|no-commit|do-not-commit|do_not_commit))(\\W|_|\\z)";

#[derive(Clone, Debug)]
pub struct CheckNocommitHook;

impl CheckNocommitHook {
    pub fn new(_config: &HookConfig) -> Result<Self, Error> {
        Ok(Self)
    }
}

fn has_nocommit(text: &[u8]) -> Option<&str> {
    let Ok(text) = std::str::from_utf8(text) else {
        // Ignore binary files
        return None;
    };

    lazy_static! {
        static ref RE: Regex = Regex::new(NOCOMMIT_REGEX).unwrap();
    }

    RE.captures(text).map(|caps| caps.get(1).unwrap().as_str())
}

#[async_trait]
impl FileHook for CheckNocommitHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        let maybe_text = match change {
            Some(change) => {
                content_manager
                    .get_file_text(ctx, change.content_id())
                    .await?
            }
            None => None,
        };

        Ok(match maybe_text {
            Some(text) => {
                if let Some(marker) = has_nocommit(text.as_ref()) {
                    let msg = format!("File contains a {} marker: {}", marker, path);
                    HookExecution::Rejected(HookRejectionInfo::new_long(
                        "File contains a nocommit marker",
                        msg,
                    ))
                } else {
                    HookExecution::Accepted
                }
            }
            None => HookExecution::Accepted,
        })
    }
}

#[async_trait]
impl ChangesetHook for CheckNocommitHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn FileContentManager,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        let message = changeset.message();

        let execution = if let Some(marker) = has_nocommit(message.as_bytes()) {
            HookExecution::Rejected(HookRejectionInfo::new_long(
                "Commit message contains a nocommit marker",
                format!("Commit message for contains a nocommit marker: {}", marker),
            ))
        } else {
            HookExecution::Accepted
        };

        Ok(execution)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_find_nocommit() {
        assert_eq!(
            Some("\x40nocommit"),
            has_nocommit("\x40nocommit".as_bytes())
        );
        assert_eq!(Some("\x40nocommit"), has_nocommit(b"foo \x40nocommit"));
        assert_eq!(None, has_nocommit(b"foo nocommit"));
    }

    #[test]
    fn test_ignore_binary() {
        assert_eq!(None, has_nocommit(b"foo \x40nocommit \x80\x81"));
    }

    #[test]
    fn test_require_word_boundaries_after() {
        assert_eq!(None, has_nocommit(b"\x40nocommitfoo"));
        assert_eq!(Some("\x40nocommit"), has_nocommit(b"foo\x40nocommit"));
        assert_eq!(Some("\x40nocommit"), has_nocommit(b"foo_\x40nocommit\""));
    }

    #[test]
    fn test_matches_underscores_before_and_after() {
        assert_eq!(Some("\x40nocommit"), has_nocommit(b"__\x40nocommit"));
        assert_eq!(Some("\x40nocommit"), has_nocommit(b"\x40nocommit__"));
    }

    #[test]
    fn test_also_matches_no_commit() {
        assert_eq!(Some("\x40no-commit"), has_nocommit(b"foo \x40no-commit"));
    }

    #[test]
    fn test_also_matches_do_not_commit() {
        assert_eq!(
            Some("\x40do-not-commit"),
            has_nocommit(b"foo \x40do-not-commit")
        );
    }

    #[test]
    fn test_also_matches_do_not_commit_with_underscores() {
        assert_eq!(
            Some("\x40do_not_commit"),
            has_nocommit(b"foo \x40do_not_commit")
        );
    }

    #[test]
    fn test_also_matches_do_not_commit_capitalized() {
        assert_eq!(
            Some("\x40DO_NOT_COMMIT"),
            has_nocommit(b"foo \x40DO_NOT_COMMIT")
        );
    }
}
