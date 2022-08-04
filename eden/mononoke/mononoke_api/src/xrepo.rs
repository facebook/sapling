/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks::BookmarkName;

use crate::specifiers::ChangesetSpecifier;

/// An aux struct to build `CandidateSelectionHint`
pub enum CandidateSelectionHintArgs {
    OnlyOrAncestorOfBookmark(BookmarkName),
    OnlyOrDescendantOfBookmark(BookmarkName),
    OnlyOrAncestorOfCommit(ChangesetSpecifier),
    OnlyOrDescendantOfCommit(ChangesetSpecifier),
    Exact(ChangesetSpecifier),
}
