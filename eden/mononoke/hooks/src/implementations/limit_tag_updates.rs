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

use crate::BookmarkHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookStateProvider;
use crate::PushAuthoredBy;

#[derive(Clone, Debug)]
pub struct LimitTagUpdatesHook {}

impl LimitTagUpdatesHook {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl BookmarkHook for LimitTagUpdatesHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        _to: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookStateProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if !bookmark.is_tag() {
            return Ok(HookExecution::Accepted);
        }

        let bookmark_state = content_manager.get_bookmark_state(ctx, bookmark).await?;
        if bookmark_state.is_new() {
            return Ok(HookExecution::Accepted);
        }
        Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
            "Modification of already existing tags is not allowed in this repository.",
            format!("Commit edits a tag {}", bookmark.as_str(),),
        )))
    }
}
