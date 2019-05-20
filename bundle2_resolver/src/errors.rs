// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashSet;

pub use failure_ext::{prelude::*, Fail};

use bookmarks::BookmarkName;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Bonsai not found for hg changeset: {:?}", _0)]
    BonsaiNotFoundForHgChangeset(HgChangesetId),
    #[fail(display = "Malformed treemanifest part: {}", _0)]
    MalformedTreemanifestPart(String),
    #[fail(display = "Pushrebase onto bookmark not found: {:?}", _0)]
    PushrebaseBookmarkNotFound(BookmarkName),
    #[fail(display = "Only one head is allowed in pushed set")]
    PushrebaseTooManyHeads,
    #[fail(
        display = "Error while uploading data for changesets, hashes: {:?}",
        _0
    )]
    WhileUploadingData(Vec<HgChangesetId>),
    #[fail(
        display = "No common root found between: bookmark:{:?} roots:{:?}",
        _0, _1
    )]
    PushrebaseNoCommonRoot(BookmarkName, HashSet<ChangesetId>),
    #[fail(display = "Repo is marked as read-only: {}", _0)]
    RepoReadOnly(String),
}
