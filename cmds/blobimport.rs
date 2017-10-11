// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(conservative_impl_trait)]

extern crate clap;
#[macro_use]
extern crate error_chain;
extern crate futures;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate slog_glog_fmt;
extern crate slog_term;
extern crate tokio_core;

extern crate blobrepo;
extern crate blobstore;
extern crate bytes;
extern crate fileblob;
extern crate fileheads;
extern crate futures_cpupool;
extern crate futures_ext;
extern crate heads;
#[macro_use]
extern crate lazy_static;
extern crate manifoldblob;
extern crate mercurial;
extern crate mercurial_types;
extern crate rocksblob;
extern crate services;
#[macro_use]
extern crate stats;

extern crate rocksdb;
extern crate serde;

extern crate bincode;

use std::error;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use bytes::Bytes;

use futures::{Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};

use tokio_core::reactor::{Core, Remote};

use futures_cpupool::CpuPool;

use clap::{App, Arg, ArgMatches};
use slog::{Drain, Level, Logger};
use slog_glog_fmt::default_drain as glog_drain;

use blobstore::Blobstore;
use fileblob::Fileblob;
use manifoldblob::ManifoldBlob;
use rocksblob::Rocksblob;

use fileheads::FileHeads;
use heads::Heads;

use blobrepo::{BlobChangeset, RawNodeBlob};

use mercurial::{RevlogManifest, RevlogRepo};
use mercurial::revlog::RevIdx;
use mercurial_types::{Blob, BlobHash, Changeset, NodeHash, Parents, Type};
use mercurial_types::manifest::{Entry, Manifest};

use stats::Timeseries;

define_stats! {
    prefix = "blobimport";
    changesets: timeseries(RATE, SUM),
    heads: timeseries(RATE, SUM),
}

#[derive(Debug, Eq, PartialEq)]
enum BlobstoreType {
    Files,
    Rocksdb,
    Manifold,
}

type BBlobstore = Arc<
    Blobstore<
        Key = String,
        ValueIn = Bytes,
        ValueOut = Vec<u8>,
        Error = Error,
        GetBlob = BoxFuture<Option<Vec<u8>>, Error>,
        PutBlob = BoxFuture<(), Error>,
    >
        + Sync,
>;

fn _assert_clone<T: Clone>(_: &T) {}
fn _assert_send<T: Send>(_: &T) {}
fn _assert_sized<T: Sized>(_: &T) {}
fn _assert_static<T: 'static>(_: &T) {}
fn _assert_blobstore<T: Blobstore>(_: &T) {}

error_chain! {
    links {
        Blobrepo(::blobrepo::Error, ::blobrepo::ErrorKind);
        Mercurial(::mercurial::Error, ::mercurial::ErrorKind);
        Rocksblob(::rocksblob::Error, ::rocksblob::ErrorKind);
        FileHeads(::fileheads::Error, ::fileheads::ErrorKind);
        Fileblob(::fileblob::Error, ::fileblob::ErrorKind);
        Manifold(::manifoldblob::Error, ::manifoldblob::ErrorKind);
    }
    foreign_links {
        Io(::std::io::Error);
    }
}

impl_kv_error!(Error);

fn put_manifest_entry(
    blobstore: BBlobstore,
    entry_hash: NodeHash,
    blob: Blob<Vec<u8>>,
    parents: Parents,
) -> impl Future<Item = (), Error = Error> + Send + 'static
where
    Error: Send + 'static,
{
    let bytes = blob.into_inner()
        .ok_or("missing blob data".into())
        .map(Bytes::from)
        .into_future();
    bytes.and_then(move |bytes| {
        let nodeblob = RawNodeBlob {
            parents: parents,
            blob: BlobHash::from(bytes.as_ref()),
        };
        // TODO: (jsgf) T21597565 Convert blobimport to use blobrepo methods to name and create
        // blobs.
        let nodekey = format!("node-{}.bincode", entry_hash);
        let blobkey = format!("sha1-{}", nodeblob.blob.sha1());
        let nodeblob = bincode::serialize(&nodeblob, bincode::Bounded(4096))
            .expect("bincode serialize failed");

        // TODO: blobstore.putv?
        let node = blobstore
            .put(nodekey, Bytes::from(nodeblob))
            .map_err(Into::into);
        let blob = blobstore.put(blobkey, bytes).map_err(Into::into);

        node.join(blob).map(|_| ())
    })
}

// Copy a single manifest entry into the blobstore
// TODO: #[async]
fn copy_manifest_entry<E>(
    entry: Box<Entry<Error = E>>,
    blobstore: BBlobstore,
) -> impl Future<Item = (), Error = Error> + Send + 'static
where
    Error: From<E>,
    E: error::Error + Send + 'static,
{
    let hash = *entry.get_hash();

    let blobfuture = entry.get_raw_content().map_err(Error::from);

    blobfuture
        .join(entry.get_parents().map_err(Error::from))
        .and_then(move |(blob, parents)| {
            put_manifest_entry(blobstore, hash, blob, parents)
        })
}

fn get_stream_of_manifest_entries(
    entry: Box<Entry<Error = mercurial::Error>>,
    revlog_repo: RevlogRepo,
    cs_rev: RevIdx,
) -> Box<Stream<Item = Box<Entry<Error = mercurial::Error>>, Error = Error> + Send> {
    let revlog = match entry.get_type() {
        Type::File | Type::Executable | Type::Symlink => {
            revlog_repo.get_file_revlog(entry.get_path())
        }
        Type::Tree => revlog_repo.get_tree_revlog(entry.get_path()),
    };

    let linkrev = revlog
        .and_then(|file_revlog| {
            file_revlog.get_entry_by_nodeid(entry.get_hash())
        })
        .map(|e| e.linkrev)
        .map_err(|e| {
            Error::with_chain(e, format!("cannot get linkrev of {}", entry.get_hash()))
        });

    match linkrev {
        Ok(linkrev) => if linkrev != cs_rev {
            return futures::stream::empty().boxify();
        },
        Err(e) => {
            return futures::stream::once(Err(e)).boxify();
        }
    }

    match entry.get_type() {
        Type::File | Type::Executable | Type::Symlink => futures::stream::once(Ok(entry)).boxify(),
        Type::Tree => entry
            .get_content()
            .and_then(|content| match content {
                mercurial_types::manifest::Content::Tree(manifest) => Ok(manifest.list()),
                _ => panic!("should not happened"),
            })
            .flatten_stream()
            .map(move |entry| {
                get_stream_of_manifest_entries(entry, revlog_repo.clone(), cs_rev.clone())
            })
            .map_err(Error::from)
            .flatten()
            .chain(futures::stream::once(Ok(entry)))
            .boxify(),
    }
}

/// Copy a changeset and its manifest into the blobstore
///
/// The changeset and the manifest are straightforward - we just make literal copies of the
/// blobs into the blobstore.
///
/// The files are more complex. For each manifest, we generate a stream of entries, then flatten
/// the entry streams from all changesets into a single stream. Then each entry is filtered
/// against a set of entries that have already been copied, and any remaining are actually copied.
fn copy_changeset(
    revlog_repo: RevlogRepo,
    blobstore: BBlobstore,
    csid: NodeHash,
) -> impl Future<Item = (), Error = Error> + Send + 'static
where
    Error: Send + 'static,
{
    let put = {
        let blobstore = blobstore.clone();
        let csid = csid;

        revlog_repo
            .get_changeset_by_nodeid(&csid)
            .from_err()
            .and_then(move |cs| {
                let bcs = BlobChangeset::new(&csid, cs);

                bcs.save(blobstore).map_err(Into::into)
            })
    };

    let manifest = revlog_repo
        .get_changeset_by_nodeid(&csid)
        .join(revlog_repo.get_changelog_revlog_entry_by_nodeid(&csid))
        .from_err()
        .and_then(move |(cs, entry)| {
            let mfid = *cs.manifestid();
            let linkrev = entry.linkrev;

            // fetch the blob for the (root) manifest
            revlog_repo
                .get_manifest_blob_by_nodeid(&mfid)
                .from_err()
                .and_then(move |blob| {
                    let putmf = put_manifest_entry(
                        blobstore.clone(),
                        mfid,
                        blob.as_blob().clone(),
                        blob.parents().clone(),
                    );

                    // Get the listing of entries and fetch each of those
                    let files = RevlogManifest::new(revlog_repo.clone(), blob)
                        .map_err(|err| {
                            Error::with_chain(Error::from(err), "Parsing manifest to get list")
                        })
                        .map(|mf| mf.list().map_err(Error::from))
                        .map(|entry_stream| {
                            entry_stream
                                .map({
                                    let revlog_repo = revlog_repo.clone();
                                    move |entry| {
                                        get_stream_of_manifest_entries(
                                            entry,
                                            revlog_repo.clone(),
                                            linkrev.clone(),
                                        )
                                    }
                                })
                                .flatten()
                                .for_each(
                                    move |entry| copy_manifest_entry(entry, blobstore.clone()),
                                )
                        })
                        .into_future()
                        .flatten();

                    _assert_sized(&files);
                    // Huh? No idea why this is needed to avoid an error below.
                    let files = files.boxify();

                    putmf.join(files)
                })
        })
        .map_err(move |err| {
            Error::with_chain(err, format!("Can't copy manifest for cs {}", csid))
        });

    _assert_sized(&put);
    _assert_sized(&manifest);

    put.join(manifest).map(|_| ())
}

fn convert<H>(
    revlog: RevlogRepo,
    blobstore: BBlobstore,
    headstore: H,
    core: Core,
    cpupool: Arc<CpuPool>,
    logger: &Logger,
) -> Result<()>
where
    H: Heads<Key = String>,
    H::Error: Into<Error>,
{
    let mut core = core;

    // Generate stream of changesets. For each changeset, save the cs blob, and the manifest blob,
    // and the files.
    let changesets = revlog.changesets()
        .map_err(Error::from)
        .enumerate()
        .map({
            let blobstore = blobstore.clone();
            let revlog = revlog.clone();
            move |(seq, csid)| {
                debug!(logger, "{}: changeset {}", seq, csid);
                STATS::changesets.add_value(1);
                copy_changeset(revlog.clone(), blobstore.clone(), csid)
            }
        }) // Stream<Future<()>>
        .map(|copy| cpupool.spawn(copy))
        .buffer_unordered(100);

    let heads = revlog
        .get_heads()
        .map_err(Error::from)
        .map_err(|err| Error::with_chain(err, "Failed get heads"))
        .map(|h| {
            debug!(logger, "head {}", h);
            STATS::heads.add_value(1);
            headstore
                .add(&format!("{}", h))
                .map_err(Into::into)
                .map_err({
                    move |err| Error::with_chain(err, format!("Failed to create head {}", h))
                })
        })
        .buffer_unordered(100);

    let convert = changesets.select(heads).for_each(|_| Ok(()));

    core.run(convert)?;

    Ok(())
}

fn run_blobimport<In: Debug, Out: Debug>(
    input: In,
    output: Out,
    blobtype: BlobstoreType,
    logger: &Logger,
    postpone_compaction: bool,
) -> Result<()>
where
    In: AsRef<Path>,
    Out: AsRef<Path>,
{
    let core = tokio_core::reactor::Core::new()?;
    let cpupool = Arc::new(CpuPool::new_num_cpus());

    let repo = open_repo(&input)?;
    info!(logger, "Opening blobstore: {:?}", output);
    let blobstore = open_blobstore(&output, blobtype, &core.remote(), postpone_compaction)?;
    info!(logger, "Opening headstore: {:?}", output);
    let headstore = open_headstore(&output, &cpupool)?;

    info!(logger, "Converting: {:?}", input);
    convert(repo, blobstore, headstore, core, cpupool, logger)
}

fn open_repo<P: AsRef<Path>>(input: P) -> Result<RevlogRepo> {
    let mut input = PathBuf::from(input.as_ref());
    if !input.exists() || !input.is_dir() {
        bail!("input {:?} doesn't exist or isn't a dir", input);
    }
    input.push(".hg");

    let revlog = RevlogRepo::open(input)?;

    Ok(revlog)
}

fn open_headstore<P: AsRef<Path>>(heads: P, pool: &Arc<CpuPool>) -> Result<FileHeads<String>> {
    let mut heads = PathBuf::from(heads.as_ref());

    heads.push("heads");
    let headstore = fileheads::FileHeads::create_with_pool(heads, pool.clone())?;

    Ok(headstore)
}

fn open_blobstore<P: AsRef<Path>>(
    output: P,
    ty: BlobstoreType,
    remote: &Remote,
    postpone_compaction: bool,
) -> Result<BBlobstore> {
    let mut output = PathBuf::from(output.as_ref());
    output.push("blobs");

    let blobstore = match ty {
        BlobstoreType::Files => Fileblob::<_, Bytes>::create(output)
            .map_err(Error::from)
            .chain_err::<_, Error>(|| "Failed to open file blob store".into())?
            .arced(),
        BlobstoreType::Rocksdb => {
            let options = rocksdb::Options::new()
                .create_if_missing(true)
                .disable_auto_compaction(postpone_compaction);
            Rocksblob::open_with_options(output, options)
                .map_err(Error::from)
                .chain_err::<_, Error>(|| "Failed to open rocksdb blob store".into())?
                .arced()
        }
        BlobstoreType::Manifold => {
            let mb: ManifoldBlob<String, Bytes> = ManifoldBlob::new_may_panic("mononoke", remote);
            mb.arced()
        }
    };

    _assert_clone(&blobstore);
    _assert_send(&blobstore);
    _assert_static(&blobstore);
    _assert_blobstore(&blobstore);

    Ok(blobstore)
}

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    App::new("revlog to blob importer")
        .version("0.0.0")
        .about("make blobs")
        .args_from_usage(
            r#"
            <INPUT>                  'input revlog repo'
            <OUTPUT>                 'output blobstore RepoCtx'

            -p, --port [PORT]        'if provided the thrift server will start on this port'

            --postpone-compaction    '(rocksdb only) postpone auto compaction while importing'

            -d, --debug              'print debug level output'
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
            let mut core = tokio_core::reactor::Core::new().expect("failed to create tokio core");
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
        let output = matches.value_of("OUTPUT").unwrap();

        let blobtype = match matches.value_of("blobstore").unwrap() {
            "files" => BlobstoreType::Files,
            "rocksdb" => BlobstoreType::Rocksdb,
            "manifold" => BlobstoreType::Manifold,
            bad => panic!("unexpected blobstore type {}", bad),
        };

        let postpone_compaction = matches.is_present("postpone-compaction");

        run_blobimport(input, output, blobtype, &root_log, postpone_compaction)?;

        if matches.value_of("blobstore").unwrap() == "rocksdb" && postpone_compaction {
            let options = rocksdb::Options::new().create_if_missing(false);
            let rocksdb = rocksdb::Db::open(Path::new(output).join("blobs"), options)
                .expect("can't open rocksdb");
            info!(root_log, "compaction started");
            rocksdb.compact_range(&[], &[]);
            info!(root_log, "compaction finished");
        }

        Ok(())
    }

    if let Err(e) = run(&root_log, matches) {
        error!(root_log, "Blobimport failed"; e);
        std::process::exit(1);
    }
}
