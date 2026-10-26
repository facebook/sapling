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
use metaconfig_types::HookConfig;
use mononoke_types::BonsaiChangeset;
use regex::Regex;
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::Pushvars;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `require_commit_message_pattern` hook.
    pub(crate) struct RequireCommitMessagePatternMessages for "require_commit_message_pattern" {
        /// The commit changes a file matching `path_regexes`, but its message
        /// does not match `pattern`.
        missing(
            /// The `pattern` regex the commit message must match.
            pattern,
        ) = "Commits that change these files must have a commit message matching '${pattern}'.";
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct RequireCommitMessagePatternConfig {
    /// File patterns that the hook applies to.  If any changed file matches
    /// any of these patterns, then the commit message pattern is required.
    #[serde(default, with = "serde_regex")]
    pub(crate) path_regexes: Vec<Regex>,

    /// Pattern to search for.  If the commit touches any of the files in
    /// `file_patterns` and this pattern is not found in the commit message,
    /// the commit is blocked.
    #[serde(with = "serde_regex")]
    pub(crate) pattern: Regex,

    /// Replaces the `missing` message. Use the hook config's `messages` instead.
    #[serde(default)]
    pub(crate) message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RequireCommitMessagePatternHook {
    config: RequireCommitMessagePatternConfig,
    messages: RequireCommitMessagePatternMessages,
}

impl RequireCommitMessagePatternHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        let options: RequireCommitMessagePatternConfig = config.parse_options()?;
        let messages = RequireCommitMessagePatternMessages::builder()
            .legacy("missing", options.message.as_deref())
            .build(config);
        Ok(Self::with_messages(options, messages))
    }

    pub fn with_messages(
        config: RequireCommitMessagePatternConfig,
        messages: RequireCommitMessagePatternMessages,
    ) -> Self {
        Self { config, messages }
    }
}

#[async_trait]
impl ChangesetHook for RequireCommitMessagePatternHook {
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
        if self.config.pattern.is_match(changeset.message()) {
            return Ok(HookExecution::accepted());
        }

        if changeset.file_changes().any(|(path, _)| {
            self.config
                .path_regexes
                .iter()
                .any(|re| re.is_match(&path.to_string()))
        }) {
            Ok(HookExecution::rejected(
                self.messages
                    .missing(self.config.pattern.as_str())
                    .into_rejection("Commit message is missing information"),
            ))
        } else {
            Ok(HookExecution::accepted())
        }
    }
}
