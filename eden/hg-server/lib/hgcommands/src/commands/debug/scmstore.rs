/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;
use std::sync::Arc;

use futures::stream;

use async_runtime::stream_to_iter as block_on_stream;
use clidispatch::errors;
use edenapi::Builder;
use edenapi_types::{FileEntry, TreeEntry};
use revisionstore::{
    indexedlogdatastore::{Entry, IndexedLogDataStoreType, IndexedLogHgIdDataStore},
    scmstore::{edenapi::EdenApiAdapter, fallback::FallbackCache, BoxedReadStore, KeyStream},
    ExtStoredPolicy,
};
use types::{HgId, Key, RepoPathBuf};

use super::NoOpts;
use super::Repo;
use super::Result;
use super::IO;

pub fn run(_opts: NoOpts, io: &IO, repo: Repo) -> Result<u8> {
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
    io.write(&format!("Full tree indexedlog path: {}\n", fullpath))?;
    let tree_indexedstore = Arc::new(
        IndexedLogHgIdDataStore::new(
            fullpath,
            ExtStoredPolicy::Use,
            &config,
            IndexedLogDataStoreType::Shared,
        )
        .unwrap(),
    );

    // IndexedLog file store
    let fullpath = format!("{}/{}/indexedlogdatastore", cachepath, reponame);
    io.write(&format!("Full file indexedlog path: {}\n", fullpath))?;
    let file_indexedstore = Arc::new(
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
        extstored_policy: ExtStoredPolicy::Ignore,
    });

    // Fallback store combinator (trees)
    let tree_fallback = Arc::new(FallbackCache {
        preferred: tree_indexedstore.clone(),
        fallback: edenapi.clone() as BoxedReadStore<Key, TreeEntry>,
        write_store: Some(tree_indexedstore),
    }) as BoxedReadStore<Key, Entry>;

    // Fallback store combinator (files)
    let file_fallback = Arc::new(FallbackCache {
        preferred: file_indexedstore.clone(),
        fallback: edenapi as BoxedReadStore<Key, FileEntry>,
        write_store: Some(file_indexedstore),
    }) as BoxedReadStore<Key, Entry>;

    // Test trees
    let tree_keystrings = [
        (
            "fbcode/eden/hg-server/lib",
            "4afe9e15f6eea3b63f23f8d3b58fef8953f0a9e6",
        ),
        ("fbcode/eden", "ecaaf8b94291f4b929c3d0ce005b0dd09c9457a4"),
        (
            "fbcode/eden/hg-server/edenscmnative",
            "6770038b05025cc8ecc4e5970ed4f28029062f68",
        ),
    ];

    let mut tree_keys = vec![];
    for &(path, id) in tree_keystrings.iter() {
        tree_keys.push(Key::new(
            RepoPathBuf::from_string(path.to_owned())?,
            HgId::from_str(id)?,
        ));
    }

    let fetched_trees = block_on_stream(
        tree_fallback.fetch_stream(Box::pin(stream::iter(tree_keys)) as KeyStream<Key>),
    );

    for item in fetched_trees {
        let msg = format!(
            "tree {}\n",
            std::str::from_utf8(
                &item
                    .expect("failed to fetch tree")
                    .content()
                    .expect("failed to extract Entry content")
            )
            .expect("failed to convert to convert to string")
        );
        io.write(&msg)?;
    }

    // Test files
    let file_keystrings = [
        (
            "fbcode/eden/hg-server/lib/revisionstore/Cargo.toml",
            "4b3d9118300087262fbf6a791b437aa7b46f0c99",
        ),
        (
            "fbcode/eden/hg-server/lib/revisionstore/TARGETS",
            "41175d2d745babe9c558c4175919b3484a407bfe",
        ),
        (
            "fbcode/eden/hg-server/lib/revisionstore/src/packstore.rs",
            "0a57062893eb6fed562a612706dad17e9daed48c",
        ),
    ];

    let mut file_keys = vec![];
    for &(path, id) in file_keystrings.iter() {
        file_keys.push(Key::new(
            RepoPathBuf::from_string(path.to_owned())?,
            HgId::from_str(id)?,
        ));
    }

    let fetched_files = block_on_stream(
        file_fallback.fetch_stream(Box::pin(stream::iter(file_keys)) as KeyStream<Key>),
    );

    for item in fetched_files {
        let msg = format!(
            "file {}\n",
            std::str::from_utf8(
                &item
                    .expect("failed to fetch file")
                    .content()
                    .expect("failed to extract Entry content")
            )
            .expect("failed to convert to convert to string")
        );
        io.write(&msg)?;
    }

    Ok(0)
}

pub fn name() -> &'static str {
    "debugscmstore"
}

pub fn doc() -> &'static str {
    "test scmstore storage api"
}
