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
use crate::HookRejectionInfo;
use crate::HookStateProvider;
use crate::PushAuthoredBy;

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

    /// Message to include in the hook rejection.
    pub(crate) message: String,
}

#[derive(Clone, Debug)]
pub struct RequireCommitMessagePatternHook {
    config: RequireCommitMessagePatternConfig,
}

impl RequireCommitMessagePatternHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: RequireCommitMessagePatternConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl ChangesetHook for RequireCommitMessagePatternHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn HookStateProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        if self.config.pattern.is_match(changeset.message()) {
            return Ok(HookExecution::Accepted);
        }

        if changeset.file_changes().any(|(path, _)| {
            self.config
                .path_regexes
                .iter()
                .any(|re| re.is_match(&path.to_string()))
        }) {
            Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Commit message is missing information",
                self.config.message.clone(),
            )))
        } else {
            Ok(HookExecution::Accepted)
        }
    }
}
