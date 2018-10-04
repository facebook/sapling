// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! The `table!` macros in this module describe the schemas for these tables in SQL storage
//! (MySQL or SQLite). These descriptions are *not* the source of truth, so if the schema ever
//! changes it will need to be updated here as well.

table! {
    use diesel::sql_types::{Integer, Binary};

    streaming_changelog_chunks (repo_id, chunk_num) {
        repo_id -> Integer,
        chunk_num -> Integer,
        idx_blob_name -> Binary,
        idx_size -> Integer,
        data_blob_name -> Binary,
        data_size -> Integer,
    }
}
