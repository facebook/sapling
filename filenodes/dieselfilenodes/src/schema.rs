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

    use mercurial_types::sql_types::{DChangesetIdSql, DFileNodeIdSql};

    filenodes (repo_id, path_hash, is_tree, filenode) {
        repo_id -> Integer,
        path_hash -> Binary,
        is_tree -> Integer,
        filenode -> DFileNodeIdSql,
        linknode -> DChangesetIdSql,
        p1 -> Nullable<DFileNodeIdSql>,
        p2 -> Nullable<DFileNodeIdSql>,
        has_copyinfo -> Integer,
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

table! {
    use diesel::sql_types::{Binary, Integer};

    use mercurial_types::sql_types::DFileNodeIdSql;

    fixedcopyinfo (repo_id, frompath_hash, fromnode, is_tree) {
        repo_id -> Integer,
        frompath_hash -> Binary,
        fromnode -> DFileNodeIdSql,
        is_tree -> Integer,
        topath_hash -> Binary,
        tonode -> DFileNodeIdSql,
    }
}
