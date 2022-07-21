/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileContentManager;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;
use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use bookmarks::BookmarkName;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;

const DEFAULT_TITLE_LENGTH: usize = 80;

#[derive(Clone, Debug)]
pub struct LimitCommitMessageLength {
    display_title_length: usize,
    length_limit: usize,
}

impl LimitCommitMessageLength {
    pub fn new(config: &HookConfig) -> Result<Self, Error> {
        let display_title_length = config
            .strings
            .get("display_title_length")
            .map(|l| l.parse().context("While parsing display_title_length"))
            .transpose()?
            .unwrap_or(DEFAULT_TITLE_LENGTH);

        let length_limit = config
            .strings
            .get("length_limit")
            .ok_or_else(|| Error::msg("Required config max_length is missing"))?;

        let length_limit = length_limit.parse().context("While parsing length_limit")?;

        Ok(Self {
            display_title_length,
            length_limit,
        })
    }
}

#[async_trait]
impl ChangesetHook for LimitCommitMessageLength {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _bookmark: &BookmarkName,
        changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn FileContentManager,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        let message = changeset.message();
        let len = message.len();

        let execution = if len >= self.length_limit {
            // Try to find a title to show.
            let title = extract_title(message, self.display_title_length);

            HookExecution::Rejected(HookRejectionInfo::new_long(
                "Commit message too long",
                format!(
                    "Commit message length for '{}' ({}) exceeds length limit (>= {})",
                    title, len, self.length_limit
                ),
            ))
        } else {
            HookExecution::Accepted
        };

        Ok(execution)
    }
}

fn extract_title<'a>(message: &'a str, max_length: usize) -> &'a str {
    let message = if message.len() > max_length {
        &message[0..max_length]
    } else {
        message
    };

    message.split('\n').into_iter().next().unwrap_or("")
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_extract_title_short() {
        assert_eq!(extract_title("foo\nbar", 4), "foo");
        assert_eq!(extract_title("foo\nbar", 5), "foo");
    }

    #[test]
    fn test_extract_title_long() {
        assert_eq!(extract_title("foo\nbar", 2), "fo");
        assert_eq!(extract_title("foo\nbar", 3), "foo");
    }

    #[test]
    fn test_extract_title_exact() {
        assert_eq!(extract_title("foo", 3), "foo");
    }

    #[test]
    fn test_extract_title_empty() {
        assert_eq!(extract_title("", 3), "");
    }
}
