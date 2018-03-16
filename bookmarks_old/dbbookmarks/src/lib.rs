// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(try_from)]

extern crate bookmarks;

extern crate ascii;
extern crate failure_ext as failure;
extern crate futures;
extern crate mysql;
#[macro_use]
extern crate mysql_async;
extern crate tokio_core;

extern crate db;
extern crate futures_ext;
extern crate mercurial_types;
extern crate sendwrapper;
extern crate storage_types;

use std::convert::TryFrom;
use std::rc::Rc;

use ascii::AsciiStr;
use failure::{Error, SyncFailure};
use futures::{future, stream, Future, Stream};
use mysql_async::{Opts, Pool, Row, TransactionOptions};
use mysql_async::prelude::*;
use tokio_core::reactor::Remote;

use bookmarks::{Bookmarks, BookmarksMut};
use db::ConnectionParams;
use futures_ext::{BoxFuture, BoxFutureNonSend, BoxStream, FutureExt, StreamExt};
use mercurial_types::nodehash::HgChangesetId;
use sendwrapper::SendWrapper;
use storage_types::Version;

pub struct DbBookmarks {
    wrapper: SendWrapper<Pool>,
}

impl DbBookmarks {
    pub fn new_async(params: ConnectionParams, remote: &Remote) -> BoxFuture<Self, Error> {
        SendWrapper::new(remote, |handle| {
            Opts::try_from(params)
                .and_then(|opts| Ok(Pool::new(opts, handle)))
                .map_err(Into::into)
        }).and_then(|wrapper| Ok(DbBookmarks { wrapper }))
            .boxify()
    }
}

impl Bookmarks for DbBookmarks {
    fn get(&self, key: &AsRef<[u8]>) -> BoxFuture<Option<(HgChangesetId, Version)>, Error> {
        let key = key.as_ref().to_vec();
        self.wrapper
            .with_inner(move |pool| get_bookmark(pool, key))
            .map_err(|e| e.context("DbBookmarks get failed").into())
            .boxify()
    }

    fn keys(&self) -> BoxStream<Vec<u8>, Error> {
        self.wrapper
            .with_inner(move |pool| list_keys(pool))
            .flatten_stream()
            .map_err(|e| e.context("DbBookmarks keys failed").into())
            .boxify()
    }
}

impl BookmarksMut for DbBookmarks {
    fn set(
        &self,
        key: &AsRef<[u8]>,
        value: &HgChangesetId,
        version: &Version,
    ) -> BoxFuture<Option<Version>, Error> {
        let key = key.as_ref().to_vec();
        let value = value.clone();
        let version = version.clone();
        self.wrapper
            .with_inner(move |pool| set_bookmark(pool, key, value, version))
            .map_err(|e| e.context("DbBookmarks set failed").into())
            .boxify()
    }

    fn delete(&self, key: &AsRef<[u8]>, version: &Version) -> BoxFuture<Option<Version>, Error> {
        let key = key.as_ref().to_vec();
        let version = version.clone();
        self.wrapper
            .with_inner(move |pool| delete_bookmark(pool, key, version))
            .map_err(|e| e.context("DbBookmarks delete failed").into())
            .boxify()
    }
}

fn list_keys(pool: Rc<Pool>) -> BoxFutureNonSend<BoxStream<Vec<u8>, Error>, Error> {
    pool.get_conn()
        .and_then(|conn| conn.query("SELECT name FROM bookmarks"))
        .and_then(|res| res.collect::<(Vec<u8>,)>())
        .map(|(_, rows)| stream::iter_ok(rows.into_iter().map(|row| row.0)).boxify())
        .map_err(|e| SyncFailure::new(e).into())
        .boxify_nonsend()
}

fn get_bookmark(
    pool: Rc<Pool>,
    key: Vec<u8>,
) -> BoxFutureNonSend<Option<(HgChangesetId, Version)>, Error> {
    pool.get_conn()
        .and_then(|conn| {
            conn.prep_exec(
                "SELECT value, version FROM bookmarks WHERE name = ?",
                (key,),
            )
        })
        .and_then(|res| res.collect::<(String, u64)>())
        .map_err(|e| SyncFailure::new(e).into())
        .and_then(|(_, mut rows)| {
            if let Some((value, version)) = rows.pop() {
                let value = AsciiStr::from_ascii(&value)?;
                let value = HgChangesetId::from_ascii_str(&value)?;
                Ok(Some((value, Version::from(version))))
            } else {
                Ok(None)
            }
        })
        .boxify_nonsend()
}

fn set_bookmark(
    pool: Rc<Pool>,
    key: Vec<u8>,
    value: HgChangesetId,
    version: Version,
) -> BoxFutureNonSend<Option<Version>, Error> {
    pool.get_conn()
        // Need to use a transaction since we need to perform both a read (to get the
        // current version, if any) and a write (to set the bookmark).
        .and_then(|conn| conn.start_transaction(TransactionOptions::new()))
        .and_then({
            let key = key.clone();
            // Get current version for this bookmark (if the key is present).
            move |txn| txn.prep_exec("SELECT version FROM bookmarks WHERE name = ?", (key,))
        })
        .and_then(|res| res.collect_and_drop::<(u64,)>())
        // At this point, change the `Error` type of this combinator chain to this crate's
        // `Error` type so we can return custom errors. This means all subsequent `Future`s
        // from `mysql_async` will need `.from_err()` to convert to our `Error` type.
        .map_err(|e| SyncFailure::new(e).into())
        .and_then(move |(txn, mut rows)| {
            // Get the current and new versions for this bookmark. If the bookmark is not present,
            // default to a current version of Version::absent() and a new version of 0.
            let raw_version = rows.pop().map(|row| row.0);
            let old_version = raw_version.map(|v| Version::from(v)).unwrap_or_default();
            let new_version = raw_version.map(|v| Version::from(v+1)).unwrap_or(Version::from(0));

            // If version matches the one passed by the caller, write the new value and version.
            if version == old_version {
                let value: String = value.to_hex().into();
                txn.prep_exec(
                    "INSERT INTO bookmarks (name, value, version) \
                     VALUES (:key, :value, 0) \
                     ON DUPLICATE KEY UPDATE \
                     value = :value, version = version + 1",
                    params!(key, value),
                ).and_then(|res| res.drop_result())
                    // Commit the transaction, and return the new version.
                    .and_then(|txn| txn.commit())
                    .map_err(|e| SyncFailure::new(e).into())
                    .map(move |_| Some(new_version))
                    .boxify_nonsend()
            } else {
                future::ok(None).boxify_nonsend()
            }
        })
        .boxify_nonsend()
}

fn delete_bookmark(
    pool: Rc<Pool>,
    key: Vec<u8>,
    version: Version,
) -> BoxFutureNonSend<Option<Version>, Error> {
    pool.get_conn()
        .and_then(move |conn| {
            // Do we expect this bookmark to exist at all? (i.e., did the caller pass the
            // the absent version?) If so, then attempt the deletion and see if it succeeds.
            // Otherwise, just check to make sure the bookmark is actually absent.
            if let Version(Some(v)) = version {
                conn.prep_exec(
                    "DELETE FROM bookmarks WHERE name = :key AND version = :v",
                    params!(key, v),
                ).map(|res| if res.affected_rows() > 0 {
                        Some(Version::absent())
                    } else {
                        None
                    })
                    .boxify_nonsend()
            } else {
                // Caller passed the absent version, so this is a no-op. That said, we
                // still need to verify that the bookmark is actually not present, and if
                // it is, signal an error by returning None.
                conn.prep_exec("SELECT 1 FROM bookmarks WHERE name = ?", (key,))
                    .and_then(|res| res.collect::<Row>())
                    .map(|(_, rows)| if rows.is_empty() {
                        Some(Version::absent())
                    } else {
                        None
                    })
                    .boxify_nonsend()
            }
        })
        .map_err(|e| SyncFailure::new(e).into())
        .boxify_nonsend()
}

pub fn init_test_db() -> ConnectionParams {
    let params = db::create_test_db("mononoke_dbbookmarks").unwrap();
    let pool = mysql::Pool::new(params.clone()).unwrap();

    let _ = pool.prep_exec(
        "CREATE TABLE bookmarks (
            id INTEGER PRIMARY KEY AUTO_INCREMENT,
            name VARBINARY(256) NOT NULL,
            value VARCHAR(40) NOT NULL,
            version INTEGER UNSIGNED NOT NULL,
            created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            UNIQUE KEY (name)
        );",
        (),
    ).unwrap();

    params
}
