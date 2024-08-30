/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This hooks prevents accidental creation of new bookmarks.
//! If no config is specified, it blocks creation of all bookmarks.
//!
//! If `allow_creations_with_marker` is specified it allows creation of new bookmarks
//! for commits that have `<allow_creations_with_marker>: new_bookmark_name` at the
//! beginning of the commit message.
//! The hook passes if `comparison_prefix`new_bookmark_name equals to the name of the
//! pushed bookmark.

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

const NAMED_CAPTURE_NAME: &str = "marker_capture";

#[derive(Clone, Debug, Deserialize)]
pub struct BlockAccidentalNewBookmarkCreationConfig {
    allow_creations_with_marker: Option<AllowCreationsWithMarker>,
    #[serde(default, with = "serde_regex")]
    bypass_for_bookmarks_matching_regex: Option<Regex>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AllowCreationsWithMarker {
    marker: String,
    comparison_prefix: Option<String>,
}

#[derive(Clone, Debug)]
struct CreationAllowedWithMarkerOptions {
    marker_extraction_regex: Regex,
    marker: String,
    comparison_prefix: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BlockAccidentalNewBookmarkCreationHook {
    creation_allowed_with_marker_options: Option<CreationAllowedWithMarkerOptions>,
    bypass_for_bookmarks_matching_regex: Option<Regex>,
}

impl BlockAccidentalNewBookmarkCreationHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: BlockAccidentalNewBookmarkCreationConfig) -> Result<Self> {
        let creation_allowed_with_marker_options = if let Some(AllowCreationsWithMarker {
            marker,
            comparison_prefix,
        }) = config.allow_creations_with_marker
        {
            let marker_extraction_regex = Regex::new(&format!(
                r"^{}:\s*(?<{}>.+?)(\s|$|\n)",
                &marker, &NAMED_CAPTURE_NAME
            ))?;
            Some(CreationAllowedWithMarkerOptions {
                marker_extraction_regex,
                marker,
                comparison_prefix,
            })
        } else {
            None
        };

        Ok(Self {
            creation_allowed_with_marker_options,
            bypass_for_bookmarks_matching_regex: config.bypass_for_bookmarks_matching_regex,
        })
    }
}

fn extract_value_from_marker<'a>(
    options: &'a CreationAllowedWithMarkerOptions,
    changeset: &'a BonsaiChangeset,
) -> Option<&'a str> {
    let captures = options
        .marker_extraction_regex
        .captures(changeset.message())?;
    Some(captures.name(NAMED_CAPTURE_NAME)?.as_str())
}

#[async_trait]
impl BookmarkHook for BlockAccidentalNewBookmarkCreationHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        bookmark: &BookmarkKey,
        to: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookStateProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let bookmark_state = content_manager.get_bookmark_state(ctx, bookmark).await?;
        if !bookmark_state.is_new() {
            return Ok(HookExecution::Accepted);
        }

        if bookmark.is_tag() {
            return Ok(HookExecution::Accepted);
        }

        let bookmark_name = bookmark.as_str();

        if let Some(regex) = &self.bypass_for_bookmarks_matching_regex {
            if regex.is_match(bookmark_name) {
                return Ok(HookExecution::Accepted);
            }
        }

        if let Some(options) = &self.creation_allowed_with_marker_options {
            if let Some(value_from_marker) = extract_value_from_marker(options, to) {
                let value_to_compare = if let Some(comparison_prefix) = &options.comparison_prefix {
                    &format!("{}{}", comparison_prefix, value_from_marker)
                } else {
                    value_from_marker
                };

                if bookmark_name == value_to_compare {
                    return Ok(HookExecution::Accepted);
                }
            }

            Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Bookmark creation is restricted in this repository.",
                format!(
                    "Add \"{}: {}\" to the commit message to be able to create this branch.",
                    options.marker,
                    bookmark_name
                        .strip_prefix(
                            options
                                .comparison_prefix
                                .as_ref()
                                .map_or("", |x| x.as_str())
                        )
                        .unwrap_or(bookmark_name),
                ),
            )))
        } else {
            Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Bookmark creation is restricted in this repository.",
                "New bookmark creation is not possible in this repository.".to_string(),
            )))
        }
    }
}
