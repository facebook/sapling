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
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookStateProvider;
use crate::PushAuthoredBy;

#[derive(Clone, Debug, Deserialize)]
pub struct BlockMergeCommitsConfig {
    disable_merge_bypass_on_bookmarks: Vec<String>,
    commit_message_bypass_tag: String,
}

#[derive(Clone, Debug)]
pub struct BlockMergeCommitsHook {
    config: BlockMergeCommitsConfig,
}

impl BlockMergeCommitsHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: BlockMergeCommitsConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl ChangesetHook for BlockMergeCommitsHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn HookStateProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if !changeset.is_merge() {
            return Ok(HookExecution::Accepted);
        }

        let is_bypass_tag = changeset
            .message()
            .contains(&self.config.commit_message_bypass_tag);

        let bookmark = bookmark.to_string();

        let is_nonbypassable_bookmark = self
            .config
            .disable_merge_bypass_on_bookmarks
            .contains(&bookmark);

        match (is_bypass_tag, is_nonbypassable_bookmark) {
            (true, true) => Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Merge commit is not allowed on this bookmark despite the bypass tag in commit message",
                "This bookmark can't have merge commits".to_string(),
            ))),
            (false, _) => Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Merge commit is not allowed",
                "You must not commit merge commits".to_string(),
            ))),
            (true, false) => Ok(HookExecution::Accepted),
        }
    }
}
