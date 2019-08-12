// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
use failure_ext::Error;
use futures::future::Future;
use futures_ext::{BoxFuture, FutureExt};
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

    write DeleteCensoredBlobs(>list content_keys: String) {
        none,
        "DELETE FROM censored_contents
         WHERE content_key IN {content_keys}"
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

    pub fn delete_censored_blobs(&self, content_keys: &[String]) -> BoxFuture<(), Error> {
        let ref_vec: Vec<&String> = content_keys.iter().collect();
        DeleteCensoredBlobs::query(&self.write_connection, &ref_vec[..])
            .map_err(Error::from)
            .map(|_| ())
            .boxify()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn test_censored_store() {
        let key_a = "aaaaaaaaaaaaaaaaaaaa".to_string();
        let key_b = "bbbbbbbbbbbbbbbbbbbb".to_string();
        let key_c = "cccccccccccccccccccc".to_string();
        let key_d = "dddddddddddddddddddd".to_string();
        let task1 = "task1".to_string();
        let task2 = "task2".to_string();
        let censored_keys1 = vec![key_a.clone(), key_b.clone()];
        let censored_keys2 = vec![key_c.clone(), key_d.clone()];

        let mut rt = Runtime::new().unwrap();
        let store = SqlCensoredContentStore::with_sqlite_in_memory().unwrap();

        rt.block_on(store.insert_censored_blobs(&censored_keys1, &task1, &Timestamp::now()))
            .expect("insert failed");
        rt.block_on(store.insert_censored_blobs(&censored_keys2, &task2, &Timestamp::now()))
            .expect("insert failed");

        let res = rt
            .block_on(store.get_all_censored_blobs())
            .expect("select failed");
        assert_eq!(res.len(), 4);

        rt.block_on(store.delete_censored_blobs(&censored_keys1))
            .expect("delete failed");
        let res = rt
            .block_on(store.get_all_censored_blobs())
            .expect("select failed");

        assert_eq!(res.contains_key(&key_c), true);
        assert_eq!(res.contains_key(&key_d), true);
        assert_eq!(res.len(), 2);
    }
}
