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
use sql::{queries, Connection};
use std::collections::HashMap;

pub use sql_ext::SqlConstructors;
use std::iter::FromIterator;

#[derive(Clone)]
pub struct SqlCensoredContentStore {
    read_connection: Connection,
}

queries! {
    read GetAllCensoredBlobs() -> (String, String) {
        "SELECT content_key, task
        FROM censored_contents"
    }
}

impl SqlConstructors for SqlCensoredContentStore {
    const LABEL: &'static str = "censored_contents";

    fn from_connections(
        _write_connection: Connection,
        read_connection: Connection,
        _read_master_connection: Connection,
    ) -> Self {
        Self { read_connection }
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
}
