/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::borrow::Cow;
use std::fmt;

use anyhow::Result;
use command::RetryablePipe;
use mononoke_types::RepositoryId;
use mysql_client::query;
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
pub struct RepositoryFingerprint(String);

impl From<String> for RepositoryFingerprint {
    fn from(other: String) -> Self {
        Self(other)
    }
}

impl ToSQL for RepositoryFingerprint {
    fn to_sql_string(&self) -> Cow<str> {
        self.0.to_sql_string()
    }
}

impl RepositoryFingerprint {
    pub fn equals_clause(&self) -> RepositoryFingerprintEqualsClause<'_> {
        RepositoryFingerprintEqualsClause(self)
    }
}

pub struct RepositoryFingerprintEqualsClause<'a>(&'a RepositoryFingerprint);

impl ToSQL for RepositoryFingerprintEqualsClause<'_> {
    fn to_sql_string(&self) -> Cow<str> {
        return query!(
            "= {fingerprint}",
            fingerprint: &RepositoryFingerprint = self.0
        )
        .to_sql_string()
        .into_owned()
        .into();
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

    pub fn id(&self) -> RepositoryId {
        self.id
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn current_fingerprint(&self) -> Result<RepositoryFingerprint> {
        let mut pipe = RetryablePipe::new();
        let remote_git_retries = 2;
        let repo_url = format!(
            "ssh://git-ro.vip.facebook.com/data/gitrepos/{}.git",
            self.name()
        );

        pipe.add_retryable_command(
            vec![
                "git",
                "-c",
                "http.extraHeader='x-route-to-mononoke: 1'",
                "ls-remote",
                &repo_url,
            ],
            remote_git_retries,
        );

        pipe.add_nonretryable_command(vec!["grep", "-v", "HEAD"]);
        pipe.add_nonretryable_command(vec!["tr", "'\t'", "' '"]);
        pipe.add_nonretryable_command(vec!["sort", "-k", "2"]);
        pipe.add_nonretryable_command(vec!["sha1sum"]);
        pipe.add_nonretryable_command(vec!["awk", "{printf($1)}"]);

        Ok(pipe.spawn_commands()?.into())
    }
}
