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
    /// Messages of the `limit_tag_updates` hook.
    pub(crate) struct LimitTagUpdatesMessages for "limit_tag_updates" {
        /// A push moves an existing tag.
        tag_moved(
            /// Name of the tag.
            tag,
        ) = "Commit edits a tag ${tag}";
    }
}

#[derive(Clone, Debug)]
pub struct LimitTagUpdatesHook {
    messages: LimitTagUpdatesMessages,
}

impl LimitTagUpdatesHook {
    pub fn new(config: &HookConfig) -> Self {
        Self {
            messages: LimitTagUpdatesMessages::new(config),
        }
    }
}

#[async_trait]
impl BookmarkHook for LimitTagUpdatesHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        bookmark: &BookmarkKey,
        _to: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
        _annotated_tags: Option<&AnnotatedTags>,
    ) -> Result<HookExecution, Error> {
        if !bookmark.is_tag() {
            return Ok(HookExecution::accepted());
        }

        let bookmark_state = hook_repo.get_bookmark_state(ctx, bookmark).await?;
        if bookmark_state.is_new() {
            return Ok(HookExecution::accepted());
        }
        Ok(HookExecution::rejected(
            self.messages.tag_moved(bookmark.as_str()).into_rejection(
                "Modification of already existing tags is not allowed in this repository.",
            ),
        ))
    }
}
