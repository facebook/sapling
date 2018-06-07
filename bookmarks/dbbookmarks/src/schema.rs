// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! The `table!` macros in this module describe the schemas for these tables in SQL storage
//! (MySQL or SQLite). These descriptions are *not* the source of truth, so if the schema ever
//! changes it will need to be updated here as well.

table! {
    use diesel::sql_types::{Integer, Text};

    use mercurial_types::sql_types::HgChangesetIdSql;

    bookmarks (repo_id, name) {
        repo_id -> Integer,
        name -> Text,
        changeset_id -> HgChangesetIdSql,
    }
}
