/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::borrow::Cow;
use std::fmt;

use mononoke_types::RepositoryId;
use mysql_client::ToSQL;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepositoryName(String);

impl From<String> for RepositoryName {
    fn from(other: String) -> Self {
        Self(other)
    }
}

impl ToSQL for RepositoryName {
    fn to_sql_string(&self) -> Cow<str> {
        self.0.to_sql_string()
    }
}

impl fmt::Display for RepositoryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl RepositoryName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Repository {
    id: RepositoryId,
    name: RepositoryName,
}

impl Repository {
    pub fn new(id: RepositoryId, name: RepositoryName) -> Self {
        Repository { id, name }
    }
}
