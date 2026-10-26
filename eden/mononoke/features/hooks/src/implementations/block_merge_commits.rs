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
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::Pushvars;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `block_merge_commits` hook.
    pub(crate) struct BlockMergeCommitsMessages for "block_merge_commits" {
        /// A merge commit has the bypass tag, but `disable_merge_bypass` is set.
        merge_in_repository() = "This repository can't have merge commits";
        /// A merge commit has the bypass tag, but the bookmark is in
        /// `disable_merge_bypass_on_bookmarks`.
        merge_on_bookmark(
            /// Name of the bookmark.
            bookmark,
        ) = "This bookmark can't have merge commits";
        /// A merge commit without the bypass tag.
        merge() = "You must not commit merge commits";
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct BlockMergeCommitsConfig {
    disable_merge_bypass: bool,
    #[serde(default)]
    disable_merge_bypass_on_bookmarks: Vec<String>,
    #[serde(default)]
    commit_message_bypass_tag: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BlockMergeCommitsHook {
    config: BlockMergeCommitsConfig,
    messages: BlockMergeCommitsMessages,
}

impl BlockMergeCommitsHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Ok(Self {
            messages: BlockMergeCommitsMessages::new(config),
            ..Self::with_config(config.parse_options()?)?
        })
    }

    pub fn with_config(config: BlockMergeCommitsConfig) -> Result<Self> {
        Ok(Self {
            config,
            messages: BlockMergeCommitsMessages::default(),
        })
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
        _maybe_pushvars: Option<&'cs Pushvars>,
    ) -> Result<HookExecution, Error> {
        if !changeset.is_merge() {
            return Ok(HookExecution::accepted());
        }

        let is_bypass_tag = if let Some(tag) = &self.config.commit_message_bypass_tag {
            changeset.message().contains(tag)
        } else {
            false
        };

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
            (true, true, _) => Ok(HookExecution::rejected(self.messages.merge_in_repository().into_rejection("Merge commit is not allowed in this repository despite the bypass tag in commit message"))),
            (true, false, true) => Ok(HookExecution::rejected(self.messages.merge_on_bookmark(&bookmark).into_rejection("Merge commit is not allowed on this bookmark despite the bypass tag in commit message"))),
            (false, _, _) => Ok(HookExecution::rejected(self.messages.merge().into_rejection("Merge commit is not allowed"))),
            (true, false, false) => Ok(HookExecution::accepted()),
        }
    }
}
