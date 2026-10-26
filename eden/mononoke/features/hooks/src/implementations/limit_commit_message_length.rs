/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::Pushvars;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `limit_commit_message_length` hook.
    pub(crate) struct LimitCommitMessageLengthMessages for "limit_commit_message_length" {
        /// The commit message is at least `length_limit` bytes long.
        too_long(
            /// The start of the message's first line.
            title,
            /// Length of the message, in bytes.
            length,
            /// The limit, in bytes.
            limit,
        ) = "Commit message length for '${title}' (${length}) exceeds length limit (>= ${limit})";
    }
}

const DEFAULT_TITLE_LENGTH: usize = 80;

#[derive(Clone, Debug, Deserialize)]
pub struct LimitCommitMessageLengthConfig {
    #[serde(default)]
    display_title_length: Option<usize>,
    length_limit: usize,
}

/// Hook to block commits with messages that exceed a length limit.
#[derive(Clone, Debug)]
pub struct LimitCommitMessageLengthHook {
    config: LimitCommitMessageLengthConfig,
    messages: LimitCommitMessageLengthMessages,
}

impl LimitCommitMessageLengthHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Ok(Self {
            messages: LimitCommitMessageLengthMessages::new(config),
            ..Self::with_config(config.parse_options()?)?
        })
    }

    pub fn with_config(config: LimitCommitMessageLengthConfig) -> Result<Self> {
        Ok(Self {
            config,
            messages: LimitCommitMessageLengthMessages::default(),
        })
    }
}

#[async_trait]
impl ChangesetHook for LimitCommitMessageLengthHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        _maybe_pushvars: Option<&'cs Pushvars>,
    ) -> Result<HookExecution, Error> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }
        let message = changeset.message();
        let len = message.len();

        let execution = if len >= self.config.length_limit {
            // Try to find a title to show.
            let title = extract_title(
                message,
                self.config
                    .display_title_length
                    .unwrap_or(DEFAULT_TITLE_LENGTH),
            );

            HookExecution::rejected(
                self.messages
                    .too_long(title, len, self.config.length_limit)
                    .into_rejection("Commit message too long"),
            )
        } else {
            HookExecution::accepted()
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

    message.split('\n').next().unwrap_or("")
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_extract_title_short() {
        assert_eq!(extract_title("foo\nbar", 4), "foo");
        assert_eq!(extract_title("foo\nbar", 5), "foo");
    }

    #[mononoke::test]
    fn test_extract_title_long() {
        assert_eq!(extract_title("foo\nbar", 2), "fo");
        assert_eq!(extract_title("foo\nbar", 3), "foo");
    }

    #[mononoke::test]
    fn test_extract_title_exact() {
        assert_eq!(extract_title("foo", 3), "foo");
    }

    #[mononoke::test]
    fn test_extract_title_empty() {
        assert_eq!(extract_title("", 3), "");
    }
}
