// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! The `table!` macros in this module describe the schemas for these tables in SQL storage
//! (MySQL or SQLite). These descriptions are *not* the source of truth, so if the schema ever
//! changes it will need to be updated here as well.

table! {
    use diesel::sql_types::{Binary, Integer, Nullable};

    use mercurial_types::sql_types::{HgChangesetIdSql, HgFileNodeIdSql};

    filenodes (repo_id, path_hash, is_tree, filenode) {
        repo_id -> Integer,
        path_hash -> Binary,
        is_tree -> Integer,
        filenode -> HgFileNodeIdSql,
        linknode -> HgChangesetIdSql,
        p1 -> Nullable<HgFileNodeIdSql>,
        p2 -> Nullable<HgFileNodeIdSql>,
    }
}

table! {
    use diesel::sql_types::{Binary, Integer};

    paths (repo_id, path_hash) {
        repo_id -> Integer,
        path_hash -> Binary,
        path -> Binary,
    }
}
