/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;
use std::io::stdin;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use env_logger::Env;
use futures::prelude::*;
use serde::Serialize;
use serde_json::Deserializer;
use structopt::StructOpt;
use tokio::io;
use tokio::io::AsyncWriteExt;

use configparser::config::{ConfigSet, Options};
use edenapi::{Builder, EdenApi, Entries, Response};
use edenapi_types::{
    json::FromJson, wire::ToWire, BookmarkRequest, CommitRevlogDataRequest, FileRequest,
    HistoryRequest, TreeRequest,
};

const DEFAULT_CONFIG_FILE: &str = ".hgrc.edenapi";

#[derive(Debug, StructOpt)]
#[structopt(name = "edenapi_cli", about = "Query the EdenAPI server")]
enum Command {
    #[structopt(about = "Check whether server is reachable")]
    Health(NoRepoArgs),
    #[structopt(about = "Request files")]
    Files(Args),
    #[structopt(about = "Request file history")]
    History(Args),
    #[structopt(about = "Request individual tree nodes")]
    Trees(Args),
    #[structopt(about = "Request commit revlog data")]
    CommitRevlogData(Args),
    #[structopt(about = "Request Bookmarks")]
    Bookmarks(Args),
}

#[derive(Debug, StructOpt)]
struct NoRepoArgs {
    #[structopt(long, short, help = "hgrc file to use (default: ~/.hgrc.edenapi)")]
    config: Option<PathBuf>,
}

#[derive(Debug, StructOpt)]
struct Args {
    repo: String,
    #[structopt(long, short, help = "hgrc file to use (default: ~/.hgrc.edenapi)")]
    config: Option<PathBuf>,
}

struct Setup<R> {
    repo: String,
    client: Arc<dyn EdenApi>,
    requests: Vec<R>,
}

impl<R: FromJson + Debug> Setup<R> {
    /// Common set up for all subcommands.
    fn from_args(args: Args) -> Result<Self> {
        Ok(Self {
            repo: args.repo,
            client: init_client(args.config)?,
            requests: read_requests()?,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::from_env(Env::default().default_filter_or("info")).init();
    match Command::from_args() {
        Command::Health(args) => cmd_health(args).await,
        Command::Files(args) => cmd_files(args).await,
        Command::History(args) => cmd_history(args).await,
        Command::Trees(args) => cmd_trees(args).await,
        Command::CommitRevlogData(args) => cmd_commit_revlog_data(args).await,
        Command::Bookmarks(args) => cmd_bookmarks(args).await,
    }
}

async fn cmd_health(args: NoRepoArgs) -> Result<()> {
    let client = init_client(args.config)?;
    let meta = client.health().await?;
    log::info!("Received response from EdenAPI server:");
    println!("{:?}", &meta);
    Ok(())
}

async fn cmd_files(args: Args) -> Result<()> {
    let Setup {
        repo,
        client,
        requests,
    } = <Setup<FileRequest>>::from_args(args)?;

    for req in requests {
        log::info!("Requesting content for {} files", req.keys.len(),);

        let response = client.files(repo.clone(), req.keys).await?;
        handle_response(response).await?;
    }

    Ok(())
}

async fn cmd_bookmarks(args: Args) -> Result<()> {
    let Setup {
        repo,
        client,
        requests,
    } = <Setup<BookmarkRequest>>::from_args(args)?;
    for req in requests {
        log::info!("Requesting values for {} bookmarks", req.bookmarks.len(),);

        let response = client.bookmarks(repo.clone(), req.bookmarks).await?;
        handle_vec(response).await?;
    }

    Ok(())
}

async fn cmd_history(args: Args) -> Result<()> {
    let Setup {
        repo,
        client,
        requests,
    } = <Setup<HistoryRequest>>::from_args(args)?;

    for req in requests {
        log::info!("Requesting history for {} files", req.keys.len(),);

        let res = client.history(repo.clone(), req.keys, req.length).await?;
        handle_response_raw(res).await?;
    }

    Ok(())
}

async fn cmd_trees(args: Args) -> Result<()> {
    let Setup {
        repo,
        client,
        requests,
    } = <Setup<TreeRequest>>::from_args(args)?;

    for req in requests {
        log::info!("Requesting {} tree nodes", req.keys.len());
        log::trace!("{:?}", &req);

        let res = client.trees(repo.clone(), req.keys, None).await?;
        handle_response(res).await?;
    }

    Ok(())
}

async fn cmd_commit_revlog_data(args: Args) -> Result<()> {
    let Setup {
        repo,
        client,
        requests,
    } = <Setup<CommitRevlogDataRequest>>::from_args(args)?;

    for req in requests {
        log::info!("Requesting revlog data for {} commits", req.hgids.len());

        let res = client.commit_revlog_data(repo.clone(), req.hgids).await?;
        handle_response_raw(res).await?;
    }

    Ok(())
}

/// Handle the incoming deserialized response by reserializing it
/// and dumping it to stdout (only if stdout isn't a TTY, to avoid
/// messing up the user's terminal).
async fn handle_response<T: ToWire>(res: Response<T>) -> Result<()> {
    let buf = serialize_and_concat(res.entries).await?;
    let stats = res.stats.await?;
    log::info!("{}", &stats);

    if atty::is(atty::Stream::Stdout) {
        log::warn!("Not writing output because stdout is a TTY");
    } else {
        log::info!("Writing output to stdout");
        io::stdout().write_all(&buf).await?;
    }

    Ok(())
}

async fn handle_vec<T: ToWire>(res: Vec<T>) -> Result<()> {
    let buf = serialize_and_concat_vec(res).await?;

    if atty::is(atty::Stream::Stdout) {
        log::warn!("Not writing output because stdout is a TTY");
    } else {
        log::info!("Writing output to stdout");
        io::stdout().write_all(&buf).await?;
    }

    Ok(())
}

// TODO(meyer): Remove when all types have wire type
async fn handle_response_raw<T: Serialize>(res: Response<T>) -> Result<()> {
    let buf = serialize_and_concat_raw(res.entries).await?;
    let stats = res.stats.await?;
    log::info!("{}", &stats);

    if atty::is(atty::Stream::Stdout) {
        log::warn!("Not writing output because stdout is a TTY");
    } else {
        log::info!("Writing output to stdout");
        io::stdout().write_all(&buf).await?;
    }

    Ok(())
}

/// CBOR serialize and concatenate all items in the incoming stream.
///
/// Normally, this wouldn't be a good idea since the EdenAPI client just
/// deserialized the entries, so immediately re-serializing them is wasteful.
/// However, in this case we're explicitly trying to exercise the public API
/// of the client, including deserialization. In practice, most users will
/// never want the raw (CBOR-encoded) entries.
async fn serialize_and_concat<T: ToWire>(entries: Entries<T>) -> Result<Vec<u8>> {
    entries
        .err_into()
        .and_then(|entry| async move { Ok(serde_cbor::to_vec(&entry.to_wire())?) })
        .try_concat()
        .await
}

async fn serialize_and_concat_vec<T: ToWire>(entries: Vec<T>) -> Result<Vec<u8>> {
    let serialized = entries
        .into_iter()
        .map(|entry| serde_cbor::to_vec(&entry.to_wire()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(serialized.concat())
}

// TODO: Remove when all types have wire type
async fn serialize_and_concat_raw<T: Serialize>(entries: Entries<T>) -> Result<Vec<u8>> {
    entries
        .err_into()
        .and_then(|entry| async move { Ok(serde_cbor::to_vec(&entry)?) })
        .try_concat()
        .await
}

fn init_client(config_path: Option<PathBuf>) -> Result<Arc<dyn EdenApi>> {
    let config = load_config(config_path)?;
    Ok(Builder::from_config(&config)?.build()?)
}

fn load_config(path: Option<PathBuf>) -> Result<ConfigSet> {
    let path = path
        .or_else(|| Some(dirs::home_dir()?.join(DEFAULT_CONFIG_FILE)))
        .context("Failed to get config file path")?;

    log::debug!("Loading config from: {:?}", &path);
    let mut config = ConfigSet::new();
    let mut errors = config.load_path(path, &Options::new());

    if errors.is_empty() {
        Ok(config)
    } else {
        // Just return the last error for simplicity.
        Err(errors.pop().unwrap().into())
    }
}

fn read_requests<R: FromJson>() -> Result<Vec<R>> {
    log::info!("Reading requests as JSON from stdin...");
    Deserializer::from_reader(stdin())
        .into_iter()
        .map(|json| Ok(R::from_json(&json?)?))
        .collect()
}
