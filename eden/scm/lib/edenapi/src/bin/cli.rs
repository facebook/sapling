/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;
use std::io::stdin;
use std::path::PathBuf;

use anyhow::{Context, Result};
use env_logger::Env;
use futures::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use structopt::StructOpt;
use tokio::prelude::*;

use configparser::config::{ConfigSet, Options};
use edenapi::{Builder, Client, EdenApi, Entries, Fetch, Progress, ProgressCallback, RepoName};
use edenapi_types::{json::FromJson, CompleteTreeRequest, DataRequest, HistoryRequest};

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
    #[structopt(about = "Request complete trees")]
    CompleteTrees(Args),
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
    repo: RepoName,
    client: Client,
    req: R,
    bar: ProgressBar,
    cb: ProgressCallback,
}

impl<R: FromJson + Debug> Setup<R> {
    /// Common set up for all subcommands.
    fn from_args(args: Args) -> Result<Self> {
        let repo = args.repo.parse()?;
        let client = init_client(args.config)?;
        let req: R = read_request()?;
        let (bar, cb) = progress_bar();

        log::trace!("{:?}", &req);

        Ok(Self {
            repo,
            client,
            req,
            bar,
            cb,
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
        Command::CompleteTrees(args) => cmd_complete_trees(args).await,
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
        req,
        bar,
        cb,
    } = <Setup<DataRequest>>::from_args(args)?;

    log::info!(
        "Requesting content for {} files in repo {}",
        req.keys.len(),
        &repo
    );

    let response = client.files(repo, req.keys, Some(cb)).await?;
    handle_response(response, bar).await
}

async fn cmd_history(args: Args) -> Result<()> {
    let Setup {
        repo,
        client,
        req,
        bar,
        cb,
    } = <Setup<HistoryRequest>>::from_args(args)?;

    log::info!(
        "Requesting history for {} files in repo {}",
        req.keys.len(),
        &repo
    );

    let res = client.history(repo, req.keys, req.length, Some(cb)).await?;
    handle_response(res, bar).await
}

async fn cmd_trees(args: Args) -> Result<()> {
    let Setup {
        repo,
        client,
        req,
        bar,
        cb,
    } = <Setup<DataRequest>>::from_args(args)?;

    log::info!("Requesting {} tree nodes in repo {}", req.keys.len(), &repo);
    log::trace!("{:?}", &req);

    let res = client.trees(repo, req.keys, Some(cb)).await?;
    handle_response(res, bar).await
}

async fn cmd_complete_trees(args: Args) -> Result<()> {
    let Setup {
        repo,
        client,
        req,
        bar,
        cb,
    } = <Setup<CompleteTreeRequest>>::from_args(args)?;

    log::info!(
        "Requesting complete trees under {} root(s) in repo '{}'",
        req.mfnodes.len(),
        &repo
    );

    let res = client
        .complete_trees(
            repo,
            req.rootdir,
            req.mfnodes,
            req.basemfnodes,
            req.depth,
            Some(cb),
        )
        .await?;
    handle_response(res, bar).await
}

/// Handle the incoming deserialized response by reserializing it
/// and dumping it to stdout (only if stdout isn't a TTY, to avoid
/// messing up the user's terminal).
async fn handle_response<T: Serialize>(res: Fetch<T>, bar: ProgressBar) -> Result<()> {
    let buf = serialize_and_concat(res.entries).await?;

    let stats = res.stats.await?;
    bar.finish_at_current_pos();

    log::info!("{}", &stats);
    log::trace!("Response metadata: {:#?}", &res.meta);

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
async fn serialize_and_concat<T: Serialize>(entries: Entries<T>) -> Result<Vec<u8>> {
    entries
        .err_into()
        .and_then(|entry| async move { Ok(serde_cbor::to_vec(&entry)?) })
        .try_concat()
        .await
}

fn init_client(config_path: Option<PathBuf>) -> Result<Client> {
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

fn read_request<R: FromJson>() -> Result<R> {
    log::info!("Reading request as JSON from stdin...");
    let json = serde_json::from_reader(stdin())?;
    R::from_json(&json)
}

fn progress_bar() -> (ProgressBar, ProgressCallback) {
    let template = "Downloaded: {decimal_bytes}\n\
                    Speed: {bytes_per_sec}\n\
                    Elapsed: {elapsed_precise}";

    let style = ProgressStyle::default_spinner().template(template);
    let bar = ProgressBar::new_spinner().with_style(style);
    let cb = Box::new({
        let bar = bar.clone();
        move |prog: Progress| bar.set_position(prog.downloaded as u64)
    });

    (bar, cb)
}
