/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks::BookmarkKey;

use crate::specifiers::ChangesetSpecifier;

/// An aux struct to build `CandidateSelectionHint`
pub enum CandidateSelectionHintArgs {
    OnlyOrAncestorOfBookmark(BookmarkKey),
    OnlyOrDescendantOfBookmark(BookmarkKey),
    OnlyOrAncestorOfCommit(ChangesetSpecifier),
    OnlyOrDescendantOfCommit(ChangesetSpecifier),
    Exact(ChangesetSpecifier),
}
