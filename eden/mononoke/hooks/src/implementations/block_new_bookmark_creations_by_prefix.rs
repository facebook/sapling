/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkPrefix;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::MPath;
use serde::Deserialize;

use crate::BookmarkHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookStateProvider;
use crate::PushAuthoredBy;

#[derive(Clone, Debug, Deserialize)]
pub struct BlockNewBookmarkCreationsByPrefixConfig {
    message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BlockNewBookmarkCreationsByPrefixHook {
    config: BlockNewBookmarkCreationsByPrefixConfig,
}

impl BlockNewBookmarkCreationsByPrefixHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: BlockNewBookmarkCreationsByPrefixConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl BookmarkHook for BlockNewBookmarkCreationsByPrefixHook {
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
        // Ensure we append a trailing slash if the bookmark doesn't have one. This is because
        // we are trying to check if the bookmark matches any existing bookmarks as a path component
        // e.g. some/bookmark/ matching some/bookmark/path.
        let bookmark_prefix_str = if !bookmark.as_str().ends_with("/") {
            format!("{bookmark}/")
        } else {
            bookmark.to_string()
        };
        // Check if this bookmark itself is a path prefix of any existing bookmark
        let bookmark_prefix = BookmarkPrefix::from_str(bookmark_prefix_str.as_str())?;
        if content_manager
            .bookmark_exists_with_prefix(ctx.clone(), &bookmark_prefix)
            .await?
        {
            if let Some(message) = &self.config.message {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Invalid bookmark creation is restricted in this repository.",
                    message.clone(),
                )));
            } else {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Invalid bookmark creation is restricted in this repository.",
                    format!(
                        "Creation of bookmark \"{bookmark}\" was blocked because it exists as a path prefix of an existing bookmark",
                    ),
                )));
            }
        }
        // The current bookmark is not a path prefix of any existing bookmark, so check if any of its path
        // prefixes exist as a bookmark for this repo.
        for bookmark_prefix_path in MPath::new(bookmark_prefix_str.as_str())?.into_ancestors() {
            let bookmark_prefix_path =
                BookmarkKey::from_str(std::str::from_utf8(&bookmark_prefix_path.to_vec())?)?;
            // Check if the path ancestors of this bookmark already exist as bookmark in the repo
            if content_manager
                .get_bookmark_state(ctx, &bookmark_prefix_path)
                .await?
                .is_existing()
            {
                if let Some(message) = &self.config.message {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "Invalid bookmark creation is restricted in this repository.",
                        message.clone(),
                    )));
                } else {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "Invalid bookmark creation is restricted in this repository.",
                        format!(
                            "Creation of bookmark \"{bookmark}\" was blocked because its path prefix \"{bookmark_prefix_path}\" already exists as a bookmark",
                        ),
                    )));
                }
            }
        }

        Ok(HookExecution::Accepted)
    }
}
