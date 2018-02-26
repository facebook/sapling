// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(conservative_impl_trait)]

extern crate bincode;
extern crate bytes;
extern crate clap;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_cpupool;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_term;
extern crate tokio_core;

extern crate blobrepo;
extern crate blobstore;
extern crate changesets;
extern crate fileblob;
extern crate fileheads;
extern crate filekv;
extern crate filelinknodes;
extern crate futures_ext;
extern crate heads;
extern crate linknodes;
extern crate manifoldblob;
extern crate memheads;
extern crate mercurial;
extern crate mercurial_types;
extern crate rocksblob;
extern crate rocksdb;
extern crate services;
#[macro_use]
extern crate stats;

mod convert;
mod manifest;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::sync_channel;
use std::thread;

use bytes::Bytes;
use changesets::{ChangesetInsert, Changesets, SqliteChangesets};
use clap::{App, Arg, ArgMatches};
use failure::{Error, Result, ResultExt, SlogKVError};
use futures::{stream, Future, IntoFuture, Stream};
use futures_cpupool::CpuPool;
use slog::{Drain, Level, Logger};
use slog_glog_fmt::default_drain as glog_drain;
use stats::Timeseries;
use tokio_core::reactor::{Core, Remote};

use blobrepo::BlobChangeset;
use blobstore::Blobstore;
use fileblob::Fileblob;
use filelinknodes::FileLinknodes;
use futures_ext::{BoxFuture, FutureExt};
use linknodes::NoopLinknodes;
use manifoldblob::ManifoldBlob;
use mercurial::{RevlogRepo, RevlogRepoOptions};
use mercurial_types::{Changeset, ChangesetId, RepositoryId};
use rocksblob::Rocksblob;

const DEFAULT_MANIFOLD_BUCKET: &str = "mononoke_prod";

define_stats! {
    prefix = "blobimport";
    changesets: timeseries(RATE, SUM),
    heads: timeseries(RATE, SUM),
    duplicates: timeseries(RATE, SUM),
    failures: timeseries(RATE, SUM),
    successes: timeseries(RATE, SUM),
}

#[derive(Debug, Eq, PartialEq)]
enum BlobstoreType {
    Files,
    Rocksdb,
    Manifold(String),
}

type BBlobstore = Arc<Blobstore>;

fn _assert_clone<T: Clone>(_: &T) {}
fn _assert_send<T: Send>(_: &T) {}
fn _assert_static<T: 'static>(_: &T) {}
fn _assert_blobstore<T: Blobstore>(_: &T) {}

pub(crate) enum BlobstoreEntry {
    ManifestEntry((String, Bytes)),
    Changeset(BlobChangeset),
}

fn run_blobimport<In, Out>(
    input: In,
    output: Out,
    blobtype: BlobstoreType,
    write_linknodes: bool,
    logger: &Logger,
    postpone_compaction: bool,
    channel_size: usize,
    skip: Option<u64>,
    commits_limit: Option<u64>,
    max_blob_size: Option<usize>,
    inmemory_logs_capacity: Option<usize>,
) -> Result<()>
where
    In: Into<PathBuf>,
    Out: Into<PathBuf> + Clone + std::fmt::Debug + Send + 'static,
{
    let input = input.into();
    let core = Core::new()?;
    let cpupool = Arc::new(CpuPool::new_num_cpus());

    info!(logger, "Opening headstore: {:?}", output);
    let headstore = open_headstore(output.clone(), &cpupool)?;

    if let BlobstoreType::Manifold(ref bucket) = blobtype {
        info!(logger, "Using ManifoldBlob with bucket: {:?}", bucket);
    } else {
        info!(logger, "Opening blobstore: {:?}", output);
    }

    let (sender, recv) = sync_channel::<BlobstoreEntry>(channel_size);
    // Separate thread that does all blobstore operations. Other worker threads send parsed revlog
    // data to this thread.
    let iothread = thread::Builder::new()
        .name("iothread".to_owned())
        .spawn({
            let output = output.clone();
            move || {
                let receiverstream = stream::iter_ok::<_, ()>(recv);
                let mut core = Core::new().expect("cannot create core in iothread");
                let blobstore = open_blobstore(
                    output,
                    blobtype,
                    &core.remote(),
                    postpone_compaction,
                    max_blob_size,
                )?;
                // Filter only manifest entries, because changeset entries should be unique
                let mut inserted_manifest_entries = std::collections::HashSet::new();
                let stream = receiverstream
                    .map(move |sender_helper| match sender_helper {
                        BlobstoreEntry::Changeset(bcs) => {
                            bcs.save(blobstore.clone()).from_err().boxify()
                        }
                        BlobstoreEntry::ManifestEntry((key, value)) => {
                            if inserted_manifest_entries.insert(key.clone()) {
                                blobstore.put(key.clone(), value).from_err().boxify()
                            } else {
                                STATS::duplicates.add_value(1);
                                Ok(()).into_future().boxify()
                            }
                        }
                    })
                    .map_err(|_| failure::err_msg("failure happened").into())
                    .buffer_unordered(channel_size)
                    .then(move |res: Result<()>| {
                        if res.is_err() {
                            STATS::failures.add_value(1);
                        } else {
                            STATS::successes.add_value(1);
                        }
                        res
                    });
                core.run(stream.for_each(|_| Ok(())))
            }
        })
        .expect("cannot start iothread");

    let repo = open_repo(&input, inmemory_logs_capacity)?;

    info!(logger, "Converting: {}", input.display());
    let convert_context = convert::ConvertContext {
        repo: repo.clone(),
        sender,
        headstore,
        core,
        cpupool: cpupool.clone(),
        logger: logger.clone(),
        skip: skip,
        commits_limit: commits_limit,
    };
    let res = if write_linknodes {
        info!(logger, "Opening linknodes store: {:?}", output);
        let output = output.clone().into();
        let linknodes_store = open_linknodes_store(&output, &cpupool)?;
        convert_context.convert(linknodes_store)
    } else {
        info!(logger, "--linknodes not specified, not writing linknodes");
        convert_context.convert(NoopLinknodes::new())
    };
    iothread.join().expect("failed to join io thread")?;
    res?;

    if !skip.is_none() && !commits_limit.is_none() {
        warn!(
            logger,
            "skipping filling up changesets store because --skip or --commits-limit is set"
        );
    } else {
        warn!(logger, "filling up changesets changesets store");
        let changesets = open_changesets_store(output.into())?;
        let mut core = Core::new()?;
        let fut = repo.changesets()
            .and_then(|node| {
                let node = ChangesetId::new(node);
                repo.get_changeset_by_changesetid(&node)
                    .map(move |cs| (cs, node))
            })
            .for_each(|(cs, node)| {
                let parents = cs.parents()
                    .into_iter()
                    .map(|p| ChangesetId::new(p))
                    .collect();
                let insert = ChangesetInsert {
                    repo_id: RepositoryId::new(0), // TODO(stash): real repo id
                    cs_id: node,
                    parents,
                };
                changesets.add(&insert)
            });
        core.run(fut)?;
    }
    Ok(())
}

fn open_changesets_store(mut output: PathBuf) -> Result<Arc<Changesets>> {
    output.push("changesets");
    Ok(Arc::new(SqliteChangesets::create(
        output.to_string_lossy(),
    )?))
}

fn open_repo<P: Into<PathBuf>>(
    input: P,
    inmemory_logs_capacity: Option<usize>,
) -> Result<RevlogRepo> {
    let mut input = input.into();
    if !input.exists() || !input.is_dir() {
        bail_msg!("input {} doesn't exist or isn't a dir", input.display());
    }
    input.push(".hg");

    let revlog = match inmemory_logs_capacity {
        Some(inmemory_logs_capacity) => {
            let options = RevlogRepoOptions {
                inmemory_logs_capacity,
            };
            RevlogRepo::open_with_options(input, options)?
        }
        None => RevlogRepo::open(input)?,
    };

    Ok(revlog)
}

fn open_headstore<P: Into<PathBuf>>(path: P, pool: &Arc<CpuPool>) -> Result<Box<heads::Heads>> {
    let mut heads = path.into();

    heads.push("heads");
    let headstore = fileheads::FileHeads::create_with_pool(heads, pool.clone())?;
    Ok(Box::new(headstore))
}

fn open_linknodes_store<P: Into<PathBuf>>(path: P, pool: &Arc<CpuPool>) -> Result<FileLinknodes> {
    let mut linknodes_path = path.into();
    linknodes_path.push("linknodes");
    let linknodes_store = FileLinknodes::create_with_pool(linknodes_path, pool.clone())?;
    Ok(linknodes_store)
}

fn open_blobstore<P: Into<PathBuf>>(
    output: P,
    ty: BlobstoreType,
    remote: &Remote,
    postpone_compaction: bool,
    max_blob_size: Option<usize>,
) -> Result<BBlobstore> {
    let blobstore: BBlobstore = match ty {
        BlobstoreType::Files => {
            let mut output = output.into();
            output.push("blobs");
            Arc::new(Fileblob::create(output)
                .map_err(Error::from)
                .context("Failed to open file blob store")?)
        }
        BlobstoreType::Rocksdb => {
            let mut output = output.into();
            output.push("blobs");
            let options = rocksdb::Options::new()
                .create_if_missing(true)
                .disable_auto_compaction(postpone_compaction);
            Arc::new(Rocksblob::open_with_options(output, options)
                .map_err(Error::from)
                .context("Failed to open rocksdb blob store")?)
        }
        BlobstoreType::Manifold(bucket) => {
            let mb: ManifoldBlob = ManifoldBlob::new_may_panic(bucket, remote);
            Arc::new(mb)
        }
    };

    let blobstore = if let Some(max_blob_size) = max_blob_size {
        Arc::new(LimitedBlobstore {
            blobstore,
            max_blob_size,
        })
    } else {
        blobstore
    };

    _assert_clone(&blobstore);
    _assert_send(&blobstore);
    _assert_static(&blobstore);
    _assert_blobstore(&blobstore);

    Ok(blobstore)
}

/// Blobstore that doesn't inserts blobs that are bigger than max_blob_size
struct LimitedBlobstore {
    blobstore: BBlobstore,
    max_blob_size: usize,
}

impl Blobstore for LimitedBlobstore {
    fn get(&self, key: String) -> BoxFuture<Option<Bytes>, Error> {
        self.blobstore.get(key)
    }

    fn put(&self, key: String, value: Bytes) -> BoxFuture<(), Error> {
        if value.len() >= self.max_blob_size {
            Ok(()).into_future().boxify()
        } else {
            self.blobstore.put(key, value)
        }
    }
}

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    App::new("revlog to blob importer")
        .version("0.0.0")
        .about("make blobs")
        .args_from_usage(
            r#"
            <INPUT>                  'input revlog repo'
            [OUTPUT]                 'output blobstore RepoCtx'

            -p, --port [PORT]        'if provided the thrift server will start on this port'

            --postpone-compaction    '(rocksdb only) postpone auto compaction while importing'

            -d, --debug              'print debug level output'
            --linknodes              'also generate linknodes'
            --channel-size [SIZE]    'channel size between worker and io threads. Default: 1000'
            --skip [SKIP]            'skips commits from the beginning'
            --commits-limit [LIMIT]  'import only LIMIT first commits from revlog repo'
            --max-blob-size [LIMIT]  'max size of the blob to be inserted'
            --inmemory-logs-capacity [CAPACITY]  'max number of filelogs and treelogs in memory'
        "#,
        )
        .arg(
            Arg::with_name("blobstore")
                .long("blobstore")
                .short("B")
                .takes_value(true)
                .possible_values(&["files", "rocksdb", "manifold"])
                .required(true)
                .help("blobstore type"),
        )
        .arg(
            Arg::with_name("bucket")
                .long("bucket")
                .takes_value(true)
                .help("bucket to use for manifold blobstore"),
        )
        .arg(
            Arg::with_name("in-memory-logs-capacity")
                .long("in-memory-logs-capacity")
                .takes_value(true)
                .help(
                    "how many filelogs and tree revlogs to store in memory. \
                     Lets one balance between memory usage and importing speed",
                ),
        )
}

fn start_thrift_service<'a>(logger: &Logger, matches: &ArgMatches<'a>) -> Result<()> {
    let port = match matches.value_of("port") {
        None => return Ok(()),
        Some(port) => port.parse().expect("Failed to parse port as number"),
    };

    info!(logger, "Initializing thrift server on port {}", port);

    thread::Builder::new()
        .name("thrift_service".to_owned())
        .spawn(move || {
            services::run_service_framework(
                "mononoke_server",
                port,
                0, // Disables separate status http server
            ).expect("failure while running thrift service framework")
        })
        .map(|_| ()) // detaches the thread
        .map_err(Error::from)
}

fn start_stats() -> Result<()> {
    thread::Builder::new()
        .name("stats_aggregation".to_owned())
        .spawn(move || {
            let mut core = Core::new().expect("failed to create tokio core");
            let scheduler = stats::schedule_stats_aggregation(&core.handle())
                .expect("failed to create stats aggregation scheduler");
            core.run(scheduler).expect("stats scheduler failed");
            // stats scheduler shouldn't finish successfully
            unreachable!()
        })?; // thread detached
    Ok(())
}

fn main() {
    let matches = setup_app().get_matches();

    let root_log = {
        let level = if matches.is_present("debug") {
            Level::Debug
        } else {
            Level::Info
        };

        let drain = glog_drain().filter_level(level).fuse();
        slog::Logger::root(drain, o![])
    };

    fn run<'a>(root_log: &Logger, matches: ArgMatches<'a>) -> Result<()> {
        start_thrift_service(&root_log, &matches)?;
        start_stats()?;

        let input = matches.value_of("INPUT").unwrap();
        let output = matches.value_of("OUTPUT");
        let bucket = matches
            .value_of("bucket")
            .unwrap_or(DEFAULT_MANIFOLD_BUCKET);

        let blobtype = match matches.value_of("blobstore").unwrap() {
            "files" => BlobstoreType::Files,
            "rocksdb" => BlobstoreType::Rocksdb,
            "manifold" => BlobstoreType::Manifold(bucket.to_string()),
            bad => panic!("unexpected blobstore type {}", bad),
        };

        let postpone_compaction = matches.is_present("postpone-compaction");

        let channel_size: usize = matches
            .value_of("channel-size")
            .map(|size| size.parse().expect("channel-size must be positive integer"))
            .unwrap_or(1000);

        let write_linknodes = matches.is_present("linknodes");

        run_blobimport(
            input,
            output.expect("output must be specified").to_string(),
            blobtype,
            write_linknodes,
            &root_log,
            postpone_compaction,
            channel_size,
            matches
                .value_of("skip")
                .map(|size| size.parse().expect("skip must be positive integer")),
            matches.value_of("commits-limit").map(|size| {
                size.parse()
                    .expect("commits-limit must be positive integer")
            }),
            matches.value_of("max-blob-size").map(|size| {
                size.parse()
                    .expect("max-blob-size must be positive integer")
            }),
            matches.value_of("inmemory-logs-capacity").map(|capacity| {
                capacity
                    .parse()
                    .expect("inmemory_logs_capacity must be positive integer")
            }),
        )?;

        if matches.value_of("blobstore").unwrap() == "rocksdb" && postpone_compaction {
            let options = rocksdb::Options::new().create_if_missing(false);
            let rocksdb = rocksdb::Db::open(Path::new(output.unwrap()).join("blobs"), options)
                .expect("can't open rocksdb");
            info!(root_log, "compaction started");
            rocksdb.compact_range(&[], &[]);
            info!(root_log, "compaction finished");
        }

        Ok(())
    }

    if let Err(e) = run(&root_log, matches) {
        error!(root_log, "Blobimport failed"; SlogKVError(e));
        std::process::exit(1);
    }
}
