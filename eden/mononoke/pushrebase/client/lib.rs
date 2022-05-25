/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod local;

use bookmarks_movement::BookmarkMovementError;
use bookmarks_types::BookmarkName;
use mononoke_types::BonsaiChangeset;
use pushrebase::PushrebaseOutcome;
use std::collections::HashSet;

pub use local::LocalPushrebaseClient;

#[async_trait::async_trait]
/// This trait provides an abstraction for pushrebase, which can be used to allow
/// pushrebase to happen remotely.
pub trait PushrebaseClient {
    /// Pushrebase the given changesets to the given bookmark.
    async fn pushrebase(
        &self,
        bookmark: &BookmarkName,
        // Must be a stack
        changesets: HashSet<BonsaiChangeset>,
    ) -> Result<PushrebaseOutcome, BookmarkMovementError>;
}
