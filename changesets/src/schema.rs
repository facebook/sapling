// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! The `table!` macros in this module describe the schemas for these tables in SQL storage
//! (MySQL or SQLite). These descriptions are *not* the source of truth, so if the schema ever
//! changes it will need to be updated here as well.

table! {
    use diesel::sql_types::{BigInt, Integer};

    use mononoke_types::sql_types::ChangesetIdSql;

    changesets {
        id -> BigInt,
        repo_id -> Integer,
        cs_id -> ChangesetIdSql,
        gen -> BigInt,
    }
}

table! {
    csparents (cs_id, parent_id, seq) {
        cs_id -> BigInt,
        parent_id -> BigInt,
        seq -> Integer,
    }
}

joinable!(csparents -> changesets (parent_id));
allow_tables_to_appear_in_same_query!(changesets, csparents);
