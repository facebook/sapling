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
use mononoke_types::BonsaiChangeset;

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
    /// Messages of the `block_empty_commit` hook.
    pub(crate) struct BlockEmptyCommitMessages for "block_empty_commit" {
        /// The commit has no file changes.
        empty() = "You must include file changes in your commit for it to land";
    }
}

#[derive(Clone, Debug)]
pub struct BlockEmptyCommit {
    messages: BlockEmptyCommitMessages,
}

impl BlockEmptyCommit {
    pub fn new(config: &HookConfig) -> Self {
        Self {
            messages: BlockEmptyCommitMessages::new(config),
        }
    }
}

#[async_trait]
impl ChangesetHook for BlockEmptyCommit {
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
        if changeset.file_changes_map().is_empty() {
            Ok(HookExecution::rejected(
                self.messages
                    .empty()
                    .into_rejection("Empty commit is not allowed"),
            ))
        } else {
            Ok(HookExecution::accepted())
        }
    }
}
