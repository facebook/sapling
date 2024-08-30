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
use regex::Regex;
use serde::Deserialize;

use crate::BookmarkHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookStateProvider;
use crate::PushAuthoredBy;

#[derive(Clone, Debug, Deserialize)]
pub struct BlockNewBookmarkCreationsByNameConfig {
    #[serde(with = "serde_regex")]
    blocked_bookmarks: Regex,
}

#[derive(Clone, Debug)]
pub struct BlockNewBookmarkCreationsByNameHook {
    config: BlockNewBookmarkCreationsByNameConfig,
}

impl BlockNewBookmarkCreationsByNameHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: BlockNewBookmarkCreationsByNameConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl BookmarkHook for BlockNewBookmarkCreationsByNameHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        _from: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookStateProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let bookmark_state = content_manager.get_bookmark_state(ctx, bookmark).await?;
        if !bookmark_state.is_new() {
            return Ok(HookExecution::Accepted);
        }

        if self.config.blocked_bookmarks.is_match(bookmark.as_str()) {
            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Bookmark creation is restricted in this repository.",
                format!(
                    "Creation of bookmark \"{}\" was blocked because it matched the '{}' regular expression",
                    bookmark.as_str(),
                    self.config.blocked_bookmarks.as_str(),
                ),
            )));
        }
        Ok(HookExecution::Accepted)
    }
}
