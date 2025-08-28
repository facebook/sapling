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
use crate::HookRepo;
use crate::PushAuthoredBy;

#[derive(Clone, Debug, Deserialize)]
pub struct BlockMergeCommitsConfig {
    disable_merge_bypass: bool,
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
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
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

        let is_nonbypassable_repository = self.config.disable_merge_bypass;

        match (
            is_bypass_tag,
            is_nonbypassable_repository,
            is_nonbypassable_bookmark,
        ) {
            (true, true, _) => Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Merge commit is not allowed in this repository despite the bypass tag in commit message",
                "This repository can't have merge commits".to_string(),
            ))),
            (true, false, true) => Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Merge commit is not allowed on this bookmark despite the bypass tag in commit message",
                "This bookmark can't have merge commits".to_string(),
            ))),
            (false, _, _) => Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Merge commit is not allowed",
                "You must not commit merge commits".to_string(),
            ))),
            (true, false, false) => Ok(HookExecution::Accepted),
        }
    }
}
