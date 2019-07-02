// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
use failure_ext::Error;
use futures::future::Future;
use futures_ext::BoxFuture;
use futures_ext::FutureExt;
use mononoke_types::Timestamp;
use sql::{queries, Connection};
use std::collections::HashMap;

pub use sql_ext::SqlConstructors;
use std::iter::FromIterator;

#[derive(Clone)]
pub struct SqlCensoredContentStore {
    read_connection: Connection,
    write_connection: Connection,
}

queries! {

    write InsertCensoredBlob(
        values: (content_key: String, task: String, add_timestamp: Timestamp)
    ) {
        none,
        "INSERT into censored_contents(content_key, task, add_timestamp) VALUES {values}"
    }

    read GetAllCensoredBlobs() -> (String, String) {
        "SELECT content_key, task
        FROM censored_contents"
    }

}

impl SqlConstructors for SqlCensoredContentStore {
    const LABEL: &'static str = "censored_contents";

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
        include_str!("../schemas/sqlite-censored.sql")
    }
}

impl SqlCensoredContentStore {
    pub fn get_all_censored_blobs(&self) -> BoxFuture<HashMap<String, String>, Error> {
        GetAllCensoredBlobs::query(&self.read_connection)
            .map(HashMap::from_iter)
            .boxify()
    }

    pub fn insert_censored_blobs(
        &self,
        content_keys: &Vec<String>,
        task: &String,
        add_timestamp: &Timestamp,
    ) -> impl Future<Item = (), Error = Error> {
        let censored_inserts: Vec<_> = content_keys
            .iter()
            .map(move |key| (key, task, add_timestamp))
            .collect();

        InsertCensoredBlob::query(&self.write_connection, &censored_inserts[..])
            .map_err(Error::from)
            .map(|_| ())
            .boxify()
    }
}
