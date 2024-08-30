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
use hook_manager::provider::TagType;
use mononoke_types::BonsaiChangeset;

use crate::BookmarkHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookStateProvider;
use crate::PushAuthoredBy;

#[derive(Clone, Debug)]
pub struct BlockUnannotatedTagsHook {}

impl BlockUnannotatedTagsHook {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl BookmarkHook for BlockUnannotatedTagsHook {
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

        if let TagType::LightweightTag = content_manager.get_tag_type(ctx, bookmark).await? {
            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Git unannotated tags are not allowed in this repository.",
                format!(
                    "The un-annotated tag \"{}\" is not allowed in this repository.\nUse 'git tag [ -a | -s ]' for tags you want to propagate.",
                    bookmark.as_str(),
                ),
            )));
        }
        Ok(HookExecution::Accepted)
    }
}
