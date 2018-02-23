// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use diesel::sql_types::Integer;

// XXX RepositoryId might want to be a short string like a Phabricator callsign.

/// Represents a repository. This ID is used throughout storage.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(HeapSizeOf, FromSqlRow, AsExpression)]
#[sql_type = "Integer"]
pub struct RepositoryId(i32);

impl RepositoryId {
    #[inline]
    pub const fn new(id: i32) -> Self {
        RepositoryId(id)
    }

    #[inline]
    pub fn id(&self) -> i32 {
        self.0
    }
}
