/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Notification Filter.
//!
//! Trait allowing the caller to determine if notification needs to be sent for the given payload.

use repo_update_logger::GitContentRefInfo;
use repo_update_logger::PlainBookmarkInfo;

/// Trait allowing the caller to determine if notification needs to be sent for the given payload.
pub trait NotificationFilter<T> {
    /// Check whether notification should be sent for the given payload.
    fn should_notify(&self, payload: &T) -> bool;
}

pub struct AllBookmarksFilter;

impl NotificationFilter<PlainBookmarkInfo> for AllBookmarksFilter {
    fn should_notify(&self, _payload: &PlainBookmarkInfo) -> bool {
        true
    }
}

pub struct OnlyTagsFilter;

impl NotificationFilter<PlainBookmarkInfo> for OnlyTagsFilter {
    fn should_notify(&self, payload: &PlainBookmarkInfo) -> bool {
        payload.bookmark_name.contains("tags/")
    }
}

pub struct AllContentRefsFilter;

impl NotificationFilter<GitContentRefInfo> for AllContentRefsFilter {
    fn should_notify(&self, _payload: &GitContentRefInfo) -> bool {
        true
    }
}
