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
use regex::Regex;
use serde::Deserialize;

use crate::BookmarkHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `block_new_bookmark_creations_by_name` hook.
    pub(crate) struct BlockNewBookmarkCreationsByNameMessages for "block_new_bookmark_creations_by_name" {
        /// A push tried to create a bookmark whose name matches `blocked_bookmarks`.
        blocked(
            /// Name of the bookmark.
            bookmark,
            /// The `blocked_bookmarks` regex.
            pattern,
        ) = "Creation of bookmark \"${bookmark}\" was blocked because it matched the '${pattern}' regular expression";
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct BlockNewBookmarkCreationsByNameConfig {
    #[serde(with = "serde_regex")]
    blocked_bookmarks: Regex,
    /// Replaces the `blocked` message. Use the hook config's `messages` instead.
    message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BlockNewBookmarkCreationsByNameHook {
    blocked_bookmarks: Regex,
    messages: BlockNewBookmarkCreationsByNameMessages,
}

impl BlockNewBookmarkCreationsByNameHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        let options: BlockNewBookmarkCreationsByNameConfig = config.parse_options()?;
        let messages = BlockNewBookmarkCreationsByNameMessages::builder()
            .legacy("blocked", options.message.as_deref())
            .build(config);
        Ok(Self {
            blocked_bookmarks: options.blocked_bookmarks,
            messages,
        })
    }
}

#[async_trait]
impl BookmarkHook for BlockNewBookmarkCreationsByNameHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        bookmark: &BookmarkKey,
        _from: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
        _annotated_tags: Option<&AnnotatedTags>,
    ) -> Result<HookExecution, Error> {
        let bookmark_state = hook_repo.get_bookmark_state(ctx, bookmark).await?;
        if !bookmark_state.is_new() {
            return Ok(HookExecution::accepted());
        }

        if self.blocked_bookmarks.is_match(bookmark.as_str()) {
            return Ok(HookExecution::rejected(
                self.messages
                    .blocked(bookmark.as_str(), self.blocked_bookmarks.as_str())
                    .into_rejection("Bookmark creation is restricted in this repository."),
            ));
        }
        Ok(HookExecution::accepted())
    }
}
