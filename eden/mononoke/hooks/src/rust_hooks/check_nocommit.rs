/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::CrossRepoPushSource;
use crate::FileContentManager;
use crate::FileHook;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use lazy_static::lazy_static;
use mononoke_types::BasicFileChange;
use mononoke_types::MPath;
use regex::Regex;

const NOCOMMIT_MARKER: &str = "\x40nocommit";
const NOCOMIT_REGEX: &str = "\x40nocommit(\\W|_|\\z)";

#[derive(Clone, Debug)]
pub struct CheckNocommitHook;

impl CheckNocommitHook {
    pub fn new(_config: &HookConfig) -> Result<Self, Error> {
        Ok(Self)
    }
}

fn has_nocommit(text: &[u8]) -> bool {
    let text = match std::str::from_utf8(text) {
        Ok(text) => text,
        Err(_) => {
            // Ignore binary files
            return false;
        }
    };

    lazy_static! {
        static ref RE: Regex = Regex::new(NOCOMIT_REGEX).unwrap();
    }

    RE.is_match(text)
}

#[async_trait]
impl FileHook for CheckNocommitHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        path: &'path MPath,
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
                if has_nocommit(text.as_ref()) {
                    let msg = format!("File contains a {} marker: {}", NOCOMMIT_MARKER, path);
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_find_nocommit() {
        assert!(has_nocommit(NOCOMMIT_MARKER.as_bytes()));
        assert!(has_nocommit(b"foo \x40nocommit"));
        assert!(!has_nocommit(b"foo nocommit"));
    }

    #[test]
    fn test_ignore_binary() {
        assert!(!has_nocommit(b"foo \x40nocommit \x80\x81"));
    }

    #[test]
    fn test_require_word_boundaries_after() {
        assert!(!has_nocommit(b"\x40nocommitfoo"));
        assert!(has_nocommit(b"foo\x40nocommit"));
        assert!(has_nocommit(b"foo_\x40nocommit\""));
    }

    #[test]
    fn test_matches_underscores_before_and_after() {
        assert!(has_nocommit(b"__\x40nocommit"));
        assert!(has_nocommit(b"\x40nocommit__"));
    }
}
