/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use futures::stream;

use async_runtime::stream_to_iter as block_on_stream;
use revisionstore::scmstore::{FileScmStoreBuilder, KeyStream, TreeScmStoreBuilder};
use types::{HgId, Key, RepoPathBuf};

use super::NoOpts;
use super::Repo;
use super::Result;
use super::IO;

pub fn run(_opts: NoOpts, io: &IO, repo: Repo) -> Result<u8> {
    let config = repo.config();

    // Test trees
    let mut tree_builder = TreeScmStoreBuilder::new(&config);
    tree_builder = tree_builder.suffix("manifests");
    let tree_scmstore = tree_builder.build()?;

    let tree_keystrings = [
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

    let mut tree_keys = vec![];
    for &(path, id) in tree_keystrings.iter() {
        tree_keys.push(Key::new(
            RepoPathBuf::from_string(path.to_owned())?,
            HgId::from_str(id)?,
        ));
    }

    let fetched_trees = block_on_stream(
        tree_scmstore.fetch_stream(Box::pin(stream::iter(tree_keys)) as KeyStream<Key>),
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
    let file_builder = FileScmStoreBuilder::new(&config);
    let file_scmstore = file_builder.build()?;

    let file_keystrings = [
        (
            "fbcode/eden/scm/lib/revisionstore/Cargo.toml",
            "4b3d9118300087262fbf6a791b437aa7b46f0c99",
        ),
        (
            "fbcode/eden/scm/lib/revisionstore/TARGETS",
            "41175d2d745babe9c558c4175919b3484a407bfe",
        ),
        (
            "fbcode/eden/scm/lib/revisionstore/src/packstore.rs",
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
        file_scmstore.fetch_stream(Box::pin(stream::iter(file_keys)) as KeyStream<Key>),
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
