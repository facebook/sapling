/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::AnnotatedTags;
use bookmarks::BookmarkKey;
use context::CoreContext;
use hook_manager::TagType;
use mononoke_types::BonsaiChangeset;

use crate::BookmarkHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `block_unannotated_tags` hook.
    pub(crate) struct BlockUnannotatedTagsMessages for "block_unannotated_tags" {
        /// A push creates or moves a lightweight (unannotated) tag.
        unannotated_tag(
            /// Name of the tag.
            tag,
        ) = "The un-annotated tag \"${tag}\" is not allowed in this repository.\nUse 'git tag [ -a | -s ]' for tags you want to propagate.";
    }
}

#[derive(Clone, Debug)]
pub struct BlockUnannotatedTagsHook {
    messages: BlockUnannotatedTagsMessages,
}

impl BlockUnannotatedTagsHook {
    pub fn new(config: &HookConfig) -> Self {
        Self {
            messages: BlockUnannotatedTagsMessages::new(config),
        }
    }
}

#[async_trait]
impl BookmarkHook for BlockUnannotatedTagsHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        bookmark: &BookmarkKey,
        _to: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
        annotated_tags: Option<&AnnotatedTags>,
    ) -> Result<HookExecution, Error> {
        if !bookmark.is_tag() {
            return Ok(HookExecution::accepted());
        }

        // Trust the server's in-push classification: the atomic path defers this annotated tag's mapping row to the bookmark txn, so get_tag_type would wrongly read it as lightweight.
        if annotated_tags.is_some_and(|tags| tags.contains(bookmark)) {
            return Ok(HookExecution::accepted());
        }

        if let TagType::LightweightTag = hook_repo.get_tag_type(ctx, bookmark).await? {
            return Ok(HookExecution::rejected(
                self.messages
                    .unannotated_tag(bookmark.as_str())
                    .into_rejection("Git unannotated tags are not allowed in this repository."),
            ));
        }
        Ok(HookExecution::accepted())
    }
}
