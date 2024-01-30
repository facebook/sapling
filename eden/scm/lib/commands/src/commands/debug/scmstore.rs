/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use async_runtime::block_on;
use async_runtime::stream_to_iter as block_on_stream;
use clidispatch::errors;
use clidispatch::ReqCtx;
use configloader::config::ConfigSet;
use revisionstore::scmstore::file_to_async_key_stream;
use revisionstore::scmstore::FetchMode;
use revisionstore::scmstore::FileAttributes;
use revisionstore::scmstore::FileStoreBuilder;
use revisionstore::scmstore::TreeStoreBuilder;
use types::Key;

use super::define_flags;
use super::Repo;
use super::Result;
use super::IO;

define_flags! {
    pub struct DebugScmStoreOpts {
        /// Fetch mode (file or tree)
        mode: String,

        /// Input file containing keys to fetch (hgid,path separated by newlines)
        path: String,

        /// Only check for the entity locally, don't make a remote request
        local: bool,
    }
}

enum FetchType {
    File,
    Tree,
}

pub fn run(ctx: ReqCtx<DebugScmStoreOpts>, repo: &mut Repo) -> Result<u8> {
    let mode = match ctx.opts.mode.as_ref() {
        "file" => FetchType::File,
        "tree" => FetchType::Tree,
        _ => return Err(errors::Abort("'mode' must be one of 'file' or 'tree'".into()).into()),
    };

    let keys: Vec<_> =
        block_on_stream(block_on(file_to_async_key_stream(ctx.opts.path.into()))?).collect();

    let config = repo.config();

    match mode {
        FetchType::File => fetch_files(&ctx.core.io, config, keys, ctx.opts.local)?,
        FetchType::Tree => fetch_trees(&ctx.core.io, config, keys, ctx.opts.local)?,
    }

    Ok(0)
}

fn fetch_files(io: &IO, config: &ConfigSet, keys: Vec<Key>, local: bool) -> Result<()> {
    let file_builder = FileStoreBuilder::new(&config);
    let store = file_builder.build()?;

    let mut stdout = io.output();

    let fetch_mode = if local {
        FetchMode::LocalOnly
    } else {
        FetchMode::AllowRemote
    };

    let fetch_result = store.fetch(
        keys.into_iter(),
        FileAttributes {
            content: true,
            aux_data: true,
        },
        fetch_mode,
    );

    let (found, missing, _errors) = fetch_result.consume();
    for (_, file) in found.into_iter() {
        write!(stdout, "Successfully fetched file: {:#?}\n", file)?;
    }
    for (key, _) in missing.into_iter() {
        write!(stdout, "Failed to fetch file: {:#?}\n", key)?;
    }

    Ok(())
}

fn fetch_trees(io: &IO, config: &ConfigSet, keys: Vec<Key>, local: bool) -> Result<()> {
    let mut tree_builder = TreeStoreBuilder::new(config);
    tree_builder = tree_builder.suffix("manifests");
    let store = tree_builder.build()?;

    let mut stdout = io.output();

    let fetch_mode = if local {
        FetchMode::LocalOnly
    } else {
        FetchMode::AllowRemote
    };

    let fetch_result = store.fetch_batch(keys.into_iter(), fetch_mode);

    let (found, missing, _errors) = fetch_result.consume();
    for complete in found.into_iter() {
        write!(stdout, "Successfully fetched tree: {:#?}\n", complete)?;
    }
    for incomplete in missing.into_iter() {
        write!(stdout, "Failed to fetch tree: {:#?}\n", incomplete)?;
    }

    Ok(())
}

pub fn aliases() -> &'static str {
    "debugscmstore"
}

pub fn doc() -> &'static str {
    "test file and tree fetching using scmstore"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
