// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types::{HgChangesetId, RepositoryId};

use schema::bookmarks;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[derive(Queryable, Insertable)]
#[table_name = "bookmarks"]
pub(crate) struct BookmarkRow {
    pub repo_id: RepositoryId,
    // TODO(stash): make AsciiString Insertable
    pub name: String,
    pub changeset_id: HgChangesetId,
}
