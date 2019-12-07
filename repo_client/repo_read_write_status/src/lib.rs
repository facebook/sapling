/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use anyhow::Error;
use futures::future::{err, ok};
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use sql::{queries, Connection};
use sql_ext::SqlConstructors;

use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};

use metaconfig_types::RepoReadOnly;

static DEFAULT_MSG: &str = "Defaulting to locked as the lock state isn't initialised for this repo";
static NOT_CONNECTED_MSG: &str = "Defaulting to locked as no database connection passed";
static DB_MSG: &str = "Repo is locked in DB";

#[derive(Clone)]
enum HgMononokeReadWrite {
    NoWrite,
    HgWrite,
    MononokeWrite,
}

impl From<HgMononokeReadWrite> for Value {
    fn from(read_write: HgMononokeReadWrite) -> Self {
        match read_write {
            HgMononokeReadWrite::NoWrite => Value::Int(0),
            HgMononokeReadWrite::HgWrite => Value::Int(1),
            HgMononokeReadWrite::MononokeWrite => Value::Int(2),
        }
    }
}

impl ConvIr<HgMononokeReadWrite> for HgMononokeReadWrite {
    fn new(val: Value) -> Result<Self, FromValueError> {
        match val {
            Value::Bytes(ref b) if b == &b"0" => Ok(HgMononokeReadWrite::NoWrite),
            Value::Int(0) => Ok(HgMononokeReadWrite::NoWrite),
            Value::Bytes(ref b) if b == &b"1" => Ok(HgMononokeReadWrite::HgWrite),
            Value::Int(1) => Ok(HgMononokeReadWrite::HgWrite),
            Value::Bytes(ref b) if b == &b"2" => Ok(HgMononokeReadWrite::MononokeWrite),
            Value::Int(2) => Ok(HgMononokeReadWrite::MononokeWrite),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for HgMononokeReadWrite {
    type Intermediate = HgMononokeReadWrite;
}

queries! {
    read GetReadWriteStatus(repo_name: String) -> (HgMononokeReadWrite, Option<String>) {
        "SELECT state, reason FROM repo_lock
        WHERE repo = {repo_name}"
    }
    write SetReadWriteStatus(values: (repo_name: String, state: HgMononokeReadWrite, reason: str)) {
        none,
        mysql("INSERT INTO repo_lock (repo, state, reason) VALUES {values} ON DUPLICATE KEY UPDATE state = VALUES(state), reason = VALUES(reason)")
        sqlite("INSERT OR REPLACE INTO repo_lock (repo, state, reason) VALUES {values}")
    }
}

#[derive(Clone, Debug)]
pub struct SqlRepoReadWriteStatus {
    write_connection: Connection,
    read_connection: Connection,
}

impl SqlConstructors for SqlRepoReadWriteStatus {
    const LABEL: &'static str = "repo-lock";

    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        _read_master_connection: Connection,
    ) -> Self {
        Self {
            write_connection,
            read_connection,
        }
    }

    fn get_up_query() -> &'static str {
        include_str!("../../schemas/sqlite-repo-lock.sql")
    }
}

impl SqlRepoReadWriteStatus {
    fn query_read_write_state(
        &self,
        repo_name: &String,
    ) -> impl Future<Item = Option<(HgMononokeReadWrite, Option<String>)>, Error = Error> {
        GetReadWriteStatus::query(&self.read_connection, &repo_name)
            .map(|rows| rows.into_iter().next())
    }

    fn set_state(
        &self,
        repo_name: &String,
        state: &HgMononokeReadWrite,
        reason: &String,
    ) -> impl Future<Item = bool, Error = Error> {
        SetReadWriteStatus::query(&self.write_connection, &[(&repo_name, &state, &reason)])
            .map(|res| res.affected_rows() > 0)
    }
}

#[derive(Clone, Debug)]
pub struct RepoReadWriteFetcher {
    sql_repo_read_write_status: Option<SqlRepoReadWriteStatus>,
    readonly_config: RepoReadOnly,
    repo_name: String,
}

impl RepoReadWriteFetcher {
    pub fn new(
        sql_repo_read_write_status: Option<SqlRepoReadWriteStatus>,
        readonly_config: RepoReadOnly,
        repo_name: String,
    ) -> Self {
        Self {
            sql_repo_read_write_status,
            readonly_config,
            repo_name,
        }
    }

    fn query_read_write_state(&self) -> impl Future<Item = RepoReadOnly, Error = Error> {
        match &self.sql_repo_read_write_status {
            Some(status) => status
                .query_read_write_state(&self.repo_name)
                .map(|item| match item {
                    Some((HgMononokeReadWrite::MononokeWrite, _)) => RepoReadOnly::ReadWrite,
                    Some((_, reason)) => {
                        RepoReadOnly::ReadOnly(reason.unwrap_or_else(|| DB_MSG.to_string()))
                    }
                    None => RepoReadOnly::ReadOnly(DEFAULT_MSG.to_string()),
                })
                .left_future(),
            None => ok(RepoReadOnly::ReadOnly(NOT_CONNECTED_MSG.to_string())).right_future(),
        }
    }

    pub fn readonly(&self) -> BoxFuture<RepoReadOnly, Error> {
        if self.sql_repo_read_write_status.is_some() {
            match self.readonly_config {
                RepoReadOnly::ReadOnly(ref reason) => {
                    ok(RepoReadOnly::ReadOnly(reason.clone())).boxify()
                }
                RepoReadOnly::ReadWrite => self.query_read_write_state().boxify(),
            }
        } else {
            ok(self.readonly_config.clone()).boxify()
        }
    }

    fn set_state(
        &self,
        state: &HgMononokeReadWrite,
        reason: &String,
    ) -> impl Future<Item = bool, Error = Error> {
        match &self.sql_repo_read_write_status {
            Some(status) => status
                .set_state(&self.repo_name, &state, &reason)
                .left_future(),
            None => err(Error::msg("db name is not specified")).right_future(),
        }
    }

    pub fn set_mononoke_read_write(
        &self,
        reason: &String,
    ) -> impl Future<Item = bool, Error = Error> {
        self.set_state(&HgMononokeReadWrite::MononokeWrite, &reason)
    }

    pub fn set_read_only(&self, reason: &String) -> impl Future<Item = bool, Error = Error> {
        self.set_state(&HgMononokeReadWrite::NoWrite, &reason)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use metaconfig_types::RepoReadOnly::*;

    static CONFIG_MSG: &str = "Set by config option";

    queries! {
        write InsertState(values: (repo: str, state: HgMononokeReadWrite)) {
            none,
            "REPLACE INTO repo_lock(repo, state)
            VALUES {values}"
        }

        write InsertStateWithReason(values: (repo: str, state: HgMononokeReadWrite, reason: str)) {
            none,
            "REPLACE INTO repo_lock(repo, state, reason)
            VALUES {values}"
        }
    }

    #[test]
    fn test_readonly_config_no_sqlite() {
        let fetcher =
            RepoReadWriteFetcher::new(None, ReadOnly(CONFIG_MSG.to_string()), "repo".to_string());
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(CONFIG_MSG.to_string())
        );
    }

    #[test]
    fn test_readwrite_config_no_sqlite() {
        let fetcher = RepoReadWriteFetcher::new(None, ReadWrite, "repo".to_string());
        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);
    }

    #[test]
    fn test_readonly_config_with_sqlite() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadOnly(CONFIG_MSG.to_string()),
            "repo".to_string(),
        );
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(CONFIG_MSG.to_string())
        );
    }

    #[test]
    fn test_readwrite_with_sqlite() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadWrite,
            "repo".to_string(),
        );
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(DEFAULT_MSG.to_string())
        );

        InsertState::query(
            &fetcher
                .sql_repo_read_write_status
                .clone()
                .unwrap()
                .write_connection,
            &[("repo", &HgMononokeReadWrite::MononokeWrite)],
        )
        .wait()
        .unwrap();

        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);

        InsertState::query(
            &fetcher
                .sql_repo_read_write_status
                .clone()
                .unwrap()
                .write_connection,
            &[("repo", &HgMononokeReadWrite::HgWrite)],
        )
        .wait()
        .unwrap();

        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(DB_MSG.to_string())
        );
    }

    #[test]
    fn test_readwrite_with_sqlite_and_reason() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadWrite,
            "repo".to_string(),
        );

        InsertStateWithReason::query(
            &fetcher
                .sql_repo_read_write_status
                .clone()
                .unwrap()
                .write_connection,
            &[("repo", &HgMononokeReadWrite::HgWrite, "reason123")],
        )
        .wait()
        .unwrap();

        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly("reason123".to_string())
        );
    }

    #[test]
    fn test_readwrite_with_sqlite_other_repo() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadWrite,
            "repo".to_string(),
        );
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(DEFAULT_MSG.to_string())
        );

        InsertState::query(
            &fetcher
                .sql_repo_read_write_status
                .clone()
                .unwrap()
                .write_connection,
            &[("other_repo", &HgMononokeReadWrite::MononokeWrite)],
        )
        .wait()
        .unwrap();

        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(DEFAULT_MSG.to_string())
        );

        InsertState::query(
            &fetcher
                .sql_repo_read_write_status
                .clone()
                .unwrap()
                .write_connection,
            &[("repo", &HgMononokeReadWrite::MononokeWrite)],
        )
        .wait()
        .unwrap();

        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);
    }

    #[test]
    fn test_write() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadWrite,
            "repo".to_string(),
        );
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(DEFAULT_MSG.to_string())
        );

        fetcher
            .set_mononoke_read_write(&"repo is locked".to_string())
            .wait()
            .unwrap();
        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);
    }
}
