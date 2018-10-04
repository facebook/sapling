// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use client::streaming_clone::schema::streaming_changelog_chunks;
use mercurial_types::RepositoryId;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[derive(Insertable, Queryable)]
#[table_name = "streaming_changelog_chunks"]
pub(crate) struct StreamingChangelogChunksRow {
    // Diesel doesn't support unsigned types.
    pub repo_id: RepositoryId,
    pub chunk_num: i32,
    pub idx_blob_name: Vec<u8>,
    pub idx_size: i32,
    pub data_blob_name: Vec<u8>,
    pub data_size: i32,
}
