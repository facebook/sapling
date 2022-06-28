/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileContentManager;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;
use anyhow::Error;
use async_trait::async_trait;
use bookmarks::BookmarkName;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;

#[derive(Clone, Debug)]
pub struct BlockEmptyCommit;

impl BlockEmptyCommit {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ChangesetHook for BlockEmptyCommit {
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
        if changeset.file_changes_map().is_empty() {
            Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Empty commit is not allowed",
                "You must include file changes in your commit for it to land".to_string(),
            )))
        } else {
            Ok(HookExecution::Accepted)
        }
    }
}
