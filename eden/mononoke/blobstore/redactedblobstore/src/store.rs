/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
use anyhow::Error;
use mononoke_types::Timestamp;
use sql::{queries, Connection};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use std::collections::HashMap;

#[derive(Clone)]
pub struct SqlRedactedContentStore {
    read_connection: Connection,
    write_connection: Connection,
}

queries! {

    write InsertRedactedBlobs(
        values: (content_key: String, task: String, add_timestamp: Timestamp, log_only: bool)
    ) {
        none,
        mysql(
            "INSERT INTO censored_contents(content_key, task, add_timestamp, log_only) VALUES {values}
            ON DUPLICATE KEY UPDATE task = VALUES(task), add_timestamp = VALUES(add_timestamp), log_ONLY = VALUES(log_only)
            "
        )
        sqlite(
            "REPLACE INTO censored_contents(content_key, task, add_timestamp, log_only) VALUES {values}"
        )
    }

    read GetAllRedactedBlobs() -> (String, String, Option<bool>) {
        "SELECT content_key, task, log_only
        FROM censored_contents"
    }

    write DeleteRedactedBlobs(>list content_keys: String) {
        none,
        "DELETE FROM censored_contents
         WHERE content_key IN {content_keys}"
    }
}

impl SqlConstruct for SqlRedactedContentStore {
    const LABEL: &'static str = "censored_contents";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-redacted.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlRedactedContentStore {}

#[derive(Clone, Debug)]
pub struct RedactedMetadata {
    pub task: String,
    pub log_only: bool,
}

impl SqlRedactedContentStore {
    pub async fn get_all_redacted_blobs(&self) -> Result<HashMap<String, RedactedMetadata>, Error> {
        let redacted_blobs = GetAllRedactedBlobs::query(&self.read_connection).await?;
        Ok(redacted_blobs
            .into_iter()
            .map(|(key, task, log_only)| {
                let redacted_metadata = RedactedMetadata {
                    task,
                    log_only: log_only.unwrap_or(false),
                };
                (key, redacted_metadata)
            })
            .collect())
    }

    pub async fn insert_redacted_blobs(
        &self,
        content_keys: &Vec<String>,
        task: &String,
        add_timestamp: &Timestamp,
        log_only: bool,
    ) -> Result<(), Error> {
        let log_only = &log_only;
        let redacted_inserts: Vec<_> = content_keys
            .iter()
            .map(move |key| (key, task, add_timestamp, log_only))
            .collect();

        InsertRedactedBlobs::query(&self.write_connection, &redacted_inserts[..])
            .await
            .map_err(Error::from)
            .map(|_| ())
    }

    pub async fn delete_redacted_blobs(&self, content_keys: &[String]) -> Result<(), Error> {
        DeleteRedactedBlobs::query(&self.write_connection, &content_keys[..])
            .await
            .map_err(Error::from)
            .map(|_| ())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[fbinit::test]
    async fn test_redacted_store(_fb: fbinit::FacebookInit) {
        let key_a = "aaaaaaaaaaaaaaaaaaaa".to_string();
        let key_b = "bbbbbbbbbbbbbbbbbbbb".to_string();
        let key_c = "cccccccccccccccccccc".to_string();
        let key_d = "dddddddddddddddddddd".to_string();
        let task1 = "task1".to_string();
        let task2 = "task2".to_string();
        let redacted_keys1 = vec![key_a.clone(), key_b.clone()];
        let redacted_keys2 = vec![key_c.clone(), key_d.clone()];

        let store = SqlRedactedContentStore::with_sqlite_in_memory().unwrap();

        store
            .insert_redacted_blobs(&redacted_keys1, &task1, &Timestamp::now(), false)
            .await
            .expect("insert failed");
        store
            .insert_redacted_blobs(&redacted_keys2, &task2, &Timestamp::now(), true)
            .await
            .expect("insert failed");

        let res = store.get_all_redacted_blobs().await.expect("select failed");
        assert_eq!(res.len(), 4);
        assert!(!res.get(&key_a).unwrap().log_only);
        assert!(!res.get(&key_b).unwrap().log_only);
        assert!(res.get(&key_c).unwrap().log_only);
        assert!(res.get(&key_d).unwrap().log_only);

        store
            .insert_redacted_blobs(&redacted_keys1, &task1, &Timestamp::now(), true)
            .await
            .expect("insert failed");
        let res = store.get_all_redacted_blobs().await.expect("select failed");
        assert!(res.get(&key_a).unwrap().log_only);
        assert!(res.get(&key_b).unwrap().log_only);

        store
            .delete_redacted_blobs(&redacted_keys1)
            .await
            .expect("delete failed");
        let res = store.get_all_redacted_blobs().await.expect("select failed");

        assert_eq!(res.contains_key(&key_c), true);
        assert_eq!(res.contains_key(&key_d), true);
        assert_eq!(res.len(), 2);
    }
}
