/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use metaconfig_types::HgsqlName;
use sql::mysql;
use sql::queries;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;

use metaconfig_types::RepoReadOnly;

static DEFAULT_MSG: &str = "Defaulting to locked as the lock state isn't initialised for this repo";
static NOT_CONNECTED_MSG: &str = "Defaulting to locked as no database connection passed";
static DB_MSG: &str = "Repo is locked in DB";

#[derive(Clone, mysql::OptTryFromRowField)]
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

impl SqlConstruct for SqlRepoReadWriteStatus {
    const LABEL: &'static str = "repo-lock";

    const CREATION_QUERY: &'static str = include_str!("../../schemas/sqlite-hg-repo-lock.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlRepoReadWriteStatus {}

impl SqlRepoReadWriteStatus {
    async fn query_read_write_state(
        &self,
        hgsql_name: &HgsqlName,
    ) -> Result<Option<(HgMononokeReadWrite, Option<String>)>, Error> {
        GetReadWriteStatus::query(&self.read_connection, hgsql_name.as_ref())
            .await
            .map(|rows| rows.into_iter().next())
    }

    async fn set_state(
        &self,
        hgsql_name: &HgsqlName,
        state: &HgMononokeReadWrite,
        reason: &String,
    ) -> Result<bool, Error> {
        SetReadWriteStatus::query(
            &self.write_connection,
            &[(hgsql_name.as_ref(), state, reason)],
        )
        .await
        .map(|res| res.affected_rows() > 0)
    }
}

#[derive(Clone, Debug)]
pub struct RepoReadWriteFetcher {
    sql_repo_read_write_status: Option<SqlRepoReadWriteStatus>,
    readonly_config: RepoReadOnly,
    hgsql_name: HgsqlName,
}

impl RepoReadWriteFetcher {
    pub fn new(
        sql_repo_read_write_status: Option<SqlRepoReadWriteStatus>,
        readonly_config: RepoReadOnly,
        hgsql_name: HgsqlName,
    ) -> Self {
        Self {
            sql_repo_read_write_status,
            readonly_config,
            hgsql_name,
        }
    }

    async fn query_read_write_state(&self) -> Result<RepoReadOnly, Error> {
        match &self.sql_repo_read_write_status {
            Some(status) => status
                .query_read_write_state(&self.hgsql_name)
                .await
                .map(|item| match item {
                    Some((HgMononokeReadWrite::MononokeWrite, _)) => RepoReadOnly::ReadWrite,
                    Some((_, reason)) => {
                        RepoReadOnly::ReadOnly(reason.unwrap_or_else(|| DB_MSG.to_string()))
                    }
                    None => RepoReadOnly::ReadOnly(DEFAULT_MSG.to_string()),
                }),
            None => Ok(RepoReadOnly::ReadOnly(NOT_CONNECTED_MSG.to_string())),
        }
    }

    pub async fn readonly(&self) -> Result<RepoReadOnly, Error> {
        if self.sql_repo_read_write_status.is_some() {
            match self.readonly_config {
                RepoReadOnly::ReadOnly(ref reason) => Ok(RepoReadOnly::ReadOnly(reason.clone())),
                RepoReadOnly::ReadWrite => self.query_read_write_state().await,
            }
        } else {
            Ok(self.readonly_config.clone())
        }
    }

    async fn set_state(&self, state: &HgMononokeReadWrite, reason: &String) -> Result<bool, Error> {
        match &self.sql_repo_read_write_status {
            Some(status) => status.set_state(&self.hgsql_name, state, reason).await,
            None => Err(Error::msg("db name is not specified")),
        }
    }

    pub async fn set_mononoke_read_write(&self, reason: &String) -> Result<bool, Error> {
        self.set_state(&HgMononokeReadWrite::MononokeWrite, reason)
            .await
    }

    pub async fn set_read_only(&self, reason: &String) -> Result<bool, Error> {
        self.set_state(&HgMononokeReadWrite::NoWrite, reason).await
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

    #[tokio::test]
    async fn test_readonly_config_no_sqlite() {
        let fetcher = RepoReadWriteFetcher::new(
            None,
            ReadOnly(CONFIG_MSG.to_string()),
            HgsqlName("repo".to_string()),
        );

        assert_eq!(
            fetcher.readonly().await.unwrap(),
            ReadOnly(CONFIG_MSG.to_string())
        );
    }

    #[tokio::test]
    async fn test_readwrite_config_no_sqlite() {
        let fetcher = RepoReadWriteFetcher::new(None, ReadWrite, HgsqlName("repo".to_string()));
        assert_eq!(fetcher.readonly().await.unwrap(), ReadWrite);
    }

    #[tokio::test]
    async fn test_readonly_config_with_sqlite() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadOnly(CONFIG_MSG.to_string()),
            HgsqlName("repo".to_string()),
        );
        assert_eq!(
            fetcher.readonly().await.unwrap(),
            ReadOnly(CONFIG_MSG.to_string())
        );
    }

    #[tokio::test]
    async fn test_readwrite_with_sqlite() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadWrite,
            HgsqlName("repo".to_string()),
        );
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(
            fetcher.readonly().await.unwrap(),
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
        .await
        .unwrap();

        assert_eq!(fetcher.readonly().await.unwrap(), ReadWrite);

        InsertState::query(
            &fetcher
                .sql_repo_read_write_status
                .clone()
                .unwrap()
                .write_connection,
            &[("repo", &HgMononokeReadWrite::HgWrite)],
        )
        .await
        .unwrap();

        assert_eq!(
            fetcher.readonly().await.unwrap(),
            ReadOnly(DB_MSG.to_string())
        );
    }

    #[tokio::test]
    async fn test_readwrite_with_sqlite_and_reason() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadWrite,
            HgsqlName("repo".to_string()),
        );

        InsertStateWithReason::query(
            &fetcher
                .sql_repo_read_write_status
                .clone()
                .unwrap()
                .write_connection,
            &[("repo", &HgMononokeReadWrite::HgWrite, "reason123")],
        )
        .await
        .unwrap();

        assert_eq!(
            fetcher.readonly().await.unwrap(),
            ReadOnly("reason123".to_string())
        );
    }

    #[tokio::test]
    async fn test_readwrite_with_sqlite_other_repo() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadWrite,
            HgsqlName("repo".to_string()),
        );
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(
            fetcher.readonly().await.unwrap(),
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
        .await
        .unwrap();

        assert_eq!(
            fetcher.readonly().await.unwrap(),
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
        .await
        .unwrap();

        assert_eq!(fetcher.readonly().await.unwrap(), ReadWrite);
    }

    #[tokio::test]
    async fn test_write() {
        let sql_repo_read_write_status = SqlRepoReadWriteStatus::with_sqlite_in_memory().unwrap();
        let fetcher = RepoReadWriteFetcher::new(
            Some(sql_repo_read_write_status),
            ReadWrite,
            HgsqlName("repo".to_string()),
        );
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(
            fetcher.readonly().await.unwrap(),
            ReadOnly(DEFAULT_MSG.to_string())
        );

        fetcher
            .set_mononoke_read_write(&"repo is locked".to_string())
            .await
            .unwrap();
        assert_eq!(fetcher.readonly().await.unwrap(), ReadWrite);
    }
}
