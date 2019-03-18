// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use futures::future::ok;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use sql::Connection;

use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};

use metaconfig_types::RepoReadOnly;

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
    read GetReadWriteStatus(repo_name: String) -> (HgMononokeReadWrite) {
        "SELECT state FROM repo_lock
        WHERE repo = {repo_name}"
    }
}

#[derive(Clone)]
pub struct RepoReadWriteFetcher {
    read_connection: Option<Connection>,
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
            readonly_config,
            repo_name,
        }
    }

    pub fn new(readonly_config: RepoReadOnly, repo_name: String) -> Self {
        Self {
            readonly_config,
            repo_name,
            read_connection: None,
        }
    }

    fn query_read_write_state(&self) -> BoxFuture<RepoReadOnly, Error> {
        GetReadWriteStatus::query(&self.read_connection.clone().unwrap(), &self.repo_name)
            .map(|rows| {
                match rows.first() {
                    Some(row) => match row {
                        (HgMononokeReadWrite::MononokeWrite,) => RepoReadOnly::ReadWrite,
                        _ => RepoReadOnly::ReadOnly,
                    },
                    // The repo state hasn't been initialised yet, so let's be cautious.
                    None => RepoReadOnly::ReadOnly,
                }
            })
            .boxify()
    }

    pub fn readonly(&self) -> BoxFuture<RepoReadOnly, Error> {
        if self.read_connection.is_some() {
            match self.readonly_config {
                RepoReadOnly::ReadOnly => ok(RepoReadOnly::ReadOnly).boxify(),
                RepoReadOnly::ReadWrite => self.query_read_write_state().boxify(),
            }
        } else {
            ok(self.readonly_config).boxify()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use failure::Result;
    use metaconfig_types::RepoReadOnly;
    use metaconfig_types::RepoReadOnly::*;
    use sql::rusqlite::Connection as SqliteConnection;

    queries! {
        write InsertState(values: (repo: str, state: HgMononokeReadWrite)) {
            none,
            "REPLACE INTO repo_lock(repo, state)
            VALUES {values}"
        }
    }

    impl RepoReadWriteFetcher {
        pub fn with_sqlite(readonly_config: RepoReadOnly, repo_name: String) -> Result<Self> {
            let sqlite_con = SqliteConnection::open_in_memory()?;
            sqlite_con.execute_batch(include_str!("../schemas/sqlite-repo-lock.sql"))?;

            let con = Connection::with_sqlite(sqlite_con);

            Ok(Self {
                readonly_config,
                repo_name,
                read_connection: Some(con),
            })
        }

        pub fn get_connection(&self) -> Option<Connection> {
            self.read_connection.clone()
        }
    }

    #[test]
    fn test_readonly_config_no_sqlite() {
        let fetcher = RepoReadWriteFetcher::new(ReadOnly, "repo".to_string());
        assert_eq!(fetcher.readonly().wait().unwrap(), ReadOnly);
    }

    #[test]
    fn test_readwrite_config_no_sqlite() {
        let fetcher = RepoReadWriteFetcher::new(ReadWrite, "repo".to_string());
        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);
    }

    #[test]
    fn test_readonly_config_with_sqlite() {
        let fetcher = RepoReadWriteFetcher::with_sqlite(ReadOnly, "repo".to_string()).unwrap();
        assert_eq!(fetcher.readonly().wait().unwrap(), ReadOnly);
    }

    #[test]
    fn test_readwrite_with_sqlite() {
        let fetcher = RepoReadWriteFetcher::with_sqlite(ReadWrite, "repo".to_string()).unwrap();
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(fetcher.readonly().wait().unwrap(), ReadOnly);

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

        assert_eq!(fetcher.readonly().wait().unwrap(), ReadOnly);
    }

    #[test]
    fn test_readwrite_with_sqlite_other_repo() {
        let fetcher = RepoReadWriteFetcher::with_sqlite(ReadWrite, "repo".to_string()).unwrap();
        // As the DB hasn't been populated for this row, ensure that we mark the repo as locked.
        assert_eq!(fetcher.readonly().wait().unwrap(), ReadOnly);

        InsertState::query(
            &fetcher.get_connection().unwrap(),
            &[("other_repo", &HgMononokeReadWrite::MononokeWrite)],
        )
        .wait()
        .unwrap();

        assert_eq!(fetcher.readonly().wait().unwrap(), ReadOnly);

        InsertState::query(
            &fetcher.get_connection().unwrap(),
            &[("repo", &HgMononokeReadWrite::MononokeWrite)],
        )
        .wait()
        .unwrap();

        assert_eq!(fetcher.readonly().wait().unwrap(), ReadWrite);
    }
}
