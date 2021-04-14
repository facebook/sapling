/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use async_runtime::{block_on_future as block_on, stream_to_iter as block_on_stream};
use clidispatch::errors;
use configparser::config::ConfigSet;
use revisionstore::scmstore::{
    util::file_to_async_key_stream, FileScmStoreBuilder, KeyStream, TreeScmStoreBuilder,
};
use types::Key;

use super::define_flags;
use super::Repo;
use super::Result;
use super::IO;

define_flags! {
    pub struct DebugScmStoreOpts {
        /// Run the python version of the command instead (actually runs mostly in rust, but uses store constructed for python, with legacy fallback).
        python: bool,

        /// Fetch mode (file or tree)
        mode: String,

        /// Input file containing keys to fetch (hgid,path separated by newlines)
        path: String,
    }
}

enum FetchMode {
    File,
    Tree,
}

pub fn run(opts: DebugScmStoreOpts, io: &IO, repo: Repo) -> Result<u8> {
    if opts.python {
        return Err(errors::FallbackToPython.into());
    }

    let mode = match opts.mode.as_ref() {
        "file" => FetchMode::File,
        "tree" => FetchMode::Tree,
        _ => return Err(errors::Abort("'mode' must be one of 'file' or 'tree'".into()).into()),
    };

    let key_stream =
        Box::pin(block_on(file_to_async_key_stream(opts.path.into()))?) as KeyStream<Key>;

    let config = repo.config();

    match mode {
        FetchMode::File => fetch_files(io, &config, key_stream)?,
        FetchMode::Tree => fetch_trees(io, &config, key_stream)?,
    }

    Ok(0)
}

fn fetch_files(io: &IO, config: &ConfigSet, keys: KeyStream<Key>) -> Result<()> {
    let file_builder = FileScmStoreBuilder::new(&config);
    let store = file_builder.build()?;

    let mut stdout = io.output();
    for item in block_on_stream(store.fetch_stream(keys)) {
        match item {
            Ok(file) => write!(stdout, "Successfully fetched file: {:#?}\n", file),
            Err(err) => write!(stdout, "Received fetch error: {:#?}\n", err),
        }?
    }

    Ok(())
}

fn fetch_trees(io: &IO, config: &ConfigSet, keys: KeyStream<Key>) -> Result<()> {
    let mut tree_builder = TreeScmStoreBuilder::new(config);
    tree_builder = tree_builder.suffix("manifests");
    let store = tree_builder.build()?;

    let mut stdout = io.output();
    for item in block_on_stream(store.fetch_stream(keys)) {
        match item {
            Ok(file) => write!(stdout, "Successfully fetched tree: {:#?}\n", file),
            Err(err) => write!(stdout, "Received fetch error: {:#?}\n", err),
        }?
    }

    Ok(())
}

pub fn name() -> &'static str {
    "debugscmstore"
}

pub fn doc() -> &'static str {
    "test file and tree fetching using scmstore"
}
