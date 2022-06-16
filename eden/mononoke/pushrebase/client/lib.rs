/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
mod facebook;
mod local;

use bookmarks_movement::{BookmarkKindRestrictions, BookmarkMovementError};
use bookmarks_types::BookmarkName;
use bytes::Bytes;
use hooks::CrossRepoPushSource;
use mononoke_types::BonsaiChangeset;
use pushrebase::PushrebaseOutcome;
use std::collections::{HashMap, HashSet};

#[cfg(fbcode_build)]
pub use facebook::scs::{override_certificate_paths, SCSPushrebaseClient};
pub use local::LocalPushrebaseClient;

#[async_trait::async_trait]
/// This trait provides an abstraction for pushrebase, which can be used to allow
/// pushrebase to happen remotely.
pub trait PushrebaseClient {
    /// Pushrebase the given changesets to the given bookmark.
    async fn pushrebase(
        &self,
        repo: String,
        bookmark: &BookmarkName,
        // Must be a stack
        changesets: HashSet<BonsaiChangeset>,
        pushvars: Option<&HashMap<String, Bytes>>,
        cross_repo_push_source: CrossRepoPushSource,
        bookmark_restrictions: BookmarkKindRestrictions,
    ) -> Result<PushrebaseOutcome, BookmarkMovementError>;
}
