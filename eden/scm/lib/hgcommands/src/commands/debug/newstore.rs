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
use revisionstore::{
    indexedlogdatastore::{IndexedLogDataStoreType, IndexedLogHgIdDataStore},
    newstore::{
        edenapi::EdenApiAdapter, fallback::FallbackStore, KeyStream, ReadStore,
    },
    ExtStoredPolicy,
};
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
    let cachepath = match config.get("remotefilelog", "cachepath") {
        Some(c) => c.to_string(),
        None => return Err(errors::Abort("remotefilelog.cachepath is not set".into()).into()),
    };

    // IndexedLog tree store
    let fullpath = format!("{}/{}/manifests/indexedlogdatastore", cachepath, reponame);
    io.write(&format!("Full indexedlog path: {}\n", fullpath))?;
    let indexedstore = Arc::new(
        IndexedLogHgIdDataStore::new(
            fullpath,
            ExtStoredPolicy::Use,
            &config,
            IndexedLogDataStoreType::Shared,
        )
        .unwrap(),
    );

    // EdenApi tree store
    let edenapi = Arc::new(EdenApiAdapter {
        client: Builder::from_config(config)?.build()?,
        repo: reponame,
    });

    // Fallback store combinator
    let fallback = Arc::new(FallbackStore {
        preferred: indexedstore,
        fallback: edenapi,
    });

    // Test trees
    let keystrings = [
        (
            "fbcode/eden/scm/lib",
            "4afe9e15f6eea3b63f23f8d3b58fef8953f0a9e6",
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

    let fetched = block_on_stream(block_on(
        fallback.fetch_stream(Box::pin(stream::iter(keys)) as KeyStream<Key>),
    ));

    for item in fetched {
        let msg = format!("tree {:#?}\n", item);
        io.write(&msg)?;
    }

    io.write(&"testing drop \n")?;


    Ok(0)
}

pub fn name() -> &'static str {
    "debugnewstore"
}

pub fn doc() -> &'static str {
    "test newstore storage api"
}
