/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;
use std::sync::Arc;

use futures::stream;

use async_runtime::{block_on_future as block_on, stream_to_iter as block_on_stream};
use clidispatch::errors;
use edenapi::Builder;
use revisionstore::newstore::{edenapi::EdenApiAdapter, KeyStream, ReadStore};
use types::{HgId, Key, RepoPathBuf};

use super::NoOpts;
use super::Repo;
use super::Result;
use super::IO;

pub fn run(_opts: NoOpts, io: &mut IO, repo: Repo) -> Result<u8> {
    let config = repo.config();

    let reponame = match config.get("remotefilelog", "reponame") {
        Some(c) => c.to_string(),
        None => return Err(errors::Abort("remotefilelog.reponame is not set".into()).into()),
    };

    let keystrings = [
        (
            "fbcode/eden/scm/lib",
            "b23e8fcdcdd5b07765231edc3b16c7d25fe66537",
        ),
        ("fbcode/eden", "ecaaf8b94291f4b929c3d0ce005b0dd09c9457a4"),
        (
            "fbcode/eden/scm/edenscmnative",
            "6770038b05025cc8ecc4e5970ed4f28029062f68",
        ),
    ];

    let mut keys = vec![];
    for &(path, id) in keystrings.iter() {
        keys.push(Key::new(
            RepoPathBuf::from_string(path.to_owned())?,
            HgId::from_str(id)?,
        ));
    }


    let adapter = Arc::new(EdenApiAdapter {
        client: Builder::from_config(config)?.build()?,
        repo: reponame,
    });


    let fetched = block_on_stream(block_on(
        adapter.fetch_stream(Box::pin(stream::iter(keys)) as KeyStream<Key>),
    ));


    for item in fetched {
        let msg = format!("tree {:#?}\n", item);
        io.write(&msg)?;
    }

    Ok(0)
}

pub fn name() -> &'static str {
    "debugnewstore"
}

pub fn doc() -> &'static str {
    "test newstore storage api"
}
