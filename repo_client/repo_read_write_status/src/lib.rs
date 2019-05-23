// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use failure_ext::{err_msg, Error};
use futures::future::{err, ok};
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use sql::{queries, Connection};

use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};

use metaconfig_types::RepoReadOnly;

static DEFAULT_MSG: &str = "Defaulting to locked as the lock state isn't initialised for this repo";
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
        // sqlite query currently doesn't support changing the value
        sqlite("INSERT INTO repo_lock (repo, state, reason) VALUES {values}")

    }
}

#[derive(Clone)]
pub struct RepoReadWriteFetcher {
    read_connection: Option<Connection>,
    write_connection: Option<Connection>,
    readonly_config: RepoReadOnly,
    repo_name: String,
}

impl RepoReadWriteFetcher {
    pub fn with_myrouter(
        readonly_config: RepoReadOnly,
        repo_name: String,
        tier: impl ToString,
        port: u16,
    ) -> Self {
        let mut builder = Connection::myrouter_builder();
        builder.tier(tier).port(port);

        Self {
            read_connection: Some(builder.build_read_only()),
            write_connection: Some(builder.build_read_write()),
            readonly_config,
            repo_name,
        }
    }

    pub fn new(readonly_config: RepoReadOnly, repo_name: String) -> Self {
        Self {
            readonly_config,
            repo_name,
            read_connection: None,
            write_connection: None,
        }
    }

    fn query_read_write_state(&self) -> BoxFuture<RepoReadOnly, Error> {
        GetReadWriteStatus::query(&self.read_connection.clone().unwrap(), &self.repo_name)
            .map(|rows| {
                match rows.into_iter().next() {
                    Some(row) => match row {
                        (HgMononokeReadWrite::MononokeWrite, _) => RepoReadOnly::ReadWrite,
                        (_, reason) => {
                            RepoReadOnly::ReadOnly(reason.unwrap_or_else(|| DB_MSG.to_string()))
                        }
                    },
                    // The repo state hasn't been initialised yet, so let's be cautious.
                    None => RepoReadOnly::ReadOnly(DEFAULT_MSG.to_string()),
                }
            })
            .boxify()
    }

    pub fn readonly(&self) -> BoxFuture<RepoReadOnly, Error> {
        if self.read_connection.is_some() {
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
        state: HgMononokeReadWrite,
        reason: String,
    ) -> impl Future<Item = bool, Error = Error> {
        match &self.write_connection {
            Some(connection) => {
                SetReadWriteStatus::query(&connection, &[(&self.repo_name, &state, &reason)])
                    .map(|res| res.affected_rows() > 0)
                    .left_future()
            }
            None => err(err_msg("db name is not specified")).right_future(),
        }
    }

    pub fn set_mononoke_read_write(
        &self,
        reason: String,
    ) -> impl Future<Item = bool, Error = Error> {
        self.set_state(HgMononokeReadWrite::MononokeWrite, reason)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use failure_ext::Result;
    use metaconfig_types::RepoReadOnly;
    use metaconfig_types::RepoReadOnly::*;
    use sql::rusqlite::Connection as SqliteConnection;

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

    impl RepoReadWriteFetcher {
        pub fn with_sqlite(readonly_config: RepoReadOnly, repo_name: String) -> Result<Self> {
            let sqlite_con = SqliteConnection::open_in_memory()?;
            sqlite_con.execute_batch(include_str!("../../schemas/sqlite-repo-lock.sql"))?;
            let read_con = Connection::with_sqlite(sqlite_con);
            let write_con = read_con.clone();

            Ok(Self {
                readonly_config,
                repo_name,
                read_connection: Some(read_con),
                write_connection: Some(write_con),
            })
        }

        pub fn get_connection(&self) -> Option<Connection> {
            self.read_connection.clone()
        }
    }

    #[test]
    fn test_readonly_config_no_sqlite() {
        let fetcher =
            RepoReadWriteFetcher::new(ReadOnly(CONFIG_MSG.to_string()), "repo".to_string());
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(CONFIG_MSG.to_string())
        );
    }

    #[test]
    fn test_readwrite_config_no_sqlite() {
        let fetcher = RepoReadWriteFetcher::new(ReadWrite, "repo".to_string());
        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);
    }

    #[test]
    fn test_readonly_config_with_sqlite() {
        let fetcher =
            RepoReadWriteFetcher::with_sqlite(ReadOnly(CONFIG_MSG.to_string()), "repo".to_string())
                .unwrap();
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(CONFIG_MSG.to_string())
        );
    }

    #[test]
    fn test_readwrite_with_sqlite() {
        let fetcher = RepoReadWriteFetcher::with_sqlite(ReadWrite, "repo".to_string()).unwrap();
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(DEFAULT_MSG.to_string())
        );

        InsertState::query(
            &fetcher.get_connection().unwrap(),
            &[("repo", &HgMononokeReadWrite::MononokeWrite)],
        )
        .wait()
        .unwrap();

        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);

        InsertState::query(
            &fetcher.get_connection().unwrap(),
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
        let fetcher = RepoReadWriteFetcher::with_sqlite(ReadWrite, "repo".to_string()).unwrap();

        InsertStateWithReason::query(
            &fetcher.get_connection().unwrap(),
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
        let fetcher = RepoReadWriteFetcher::with_sqlite(ReadWrite, "repo".to_string()).unwrap();
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(DEFAULT_MSG.to_string())
        );

        InsertState::query(
            &fetcher.get_connection().unwrap(),
            &[("other_repo", &HgMononokeReadWrite::MononokeWrite)],
        )
        .wait()
        .unwrap();

        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(DEFAULT_MSG.to_string())
        );

        InsertState::query(
            &fetcher.get_connection().unwrap(),
            &[("repo", &HgMononokeReadWrite::MononokeWrite)],
        )
        .wait()
        .unwrap();

        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);
    }

    #[test]
    fn test_write() {
        let fetcher = RepoReadWriteFetcher::with_sqlite(ReadWrite, "repo".to_string()).unwrap();
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(
            fetcher.readonly().wait().unwrap(),
            ReadOnly(DEFAULT_MSG.to_string())
        );

        fetcher
            .set_mononoke_read_write("repo is locked".to_string())
            .wait()
            .unwrap();
        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);
    }
}
