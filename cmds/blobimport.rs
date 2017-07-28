// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate clap;
extern crate futures;
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate slog;
extern crate slog_term;
extern crate tokio_core;

extern crate mercurial;
extern crate mercurial_types;
extern crate blobstore;
extern crate fileblob;
extern crate rocksblob;
extern crate futures_ext;
extern crate futures_cpupool;
extern crate bytes;
extern crate heads;
extern crate fileheads;
extern crate blobrepo;

#[macro_use]
extern crate serde_derive;
extern crate serde;

extern crate bincode;

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;

use futures::{Future, IntoFuture, Stream};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures_ext::StreamExt;

use futures_cpupool::CpuPool;

use clap::{App, Arg};
use slog::{Drain, Level, LevelFilter, Logger};

use blobstore::Blobstore;
use fileblob::Fileblob;
use rocksblob::Rocksblob;

use heads::Heads;
use fileheads::FileHeads;

use blobrepo::BlobChangeset;

use mercurial::{RevlogManifest, RevlogRepo};
use mercurial_types::{Changeset, NodeHash, Parents, hash};
use mercurial_types::manifest::{Content, Entry, Manifest};

#[derive(Debug, Copy, Clone)]
#[derive(Serialize, Deserialize)]
pub struct NodeBlob {
    parents: Parents,
    blob: hash::Sha1,
}

#[derive(Debug, Eq, PartialEq)]
enum BlobstoreType {
    Files,
    Rocksdb,
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
fn _assert_static<T: 'static>(_: &T) {}
fn _assert_blobstore<T: Blobstore>(_: &T) {}

error_chain! {
    links {
        Blobrepo(::blobrepo::Error, ::blobrepo::ErrorKind);
        Mercurial(::mercurial::Error, ::mercurial::ErrorKind);
        Rocksblob(::rocksblob::Error, ::rocksblob::ErrorKind);
        FileHeads(::fileheads::Error, ::fileheads::ErrorKind);
        Fileblob(::fileblob::Error, ::fileblob::ErrorKind);
    }
    foreign_links {
        Io(::std::io::Error);
    }
}

// Copy a single manifest entry into the blobstore
// TODO: recast as `impl Future<...>` - remove most of these type constraints (which are mostly
// for BoxFuture)
// TODO: #[async]
fn copy_file<E>(entry: &Entry<Error = E>, blobstore: BBlobstore) -> BoxFuture<(), Error>
where
    Error: From<E>,
    E: Send + 'static,
{
    let hash = *entry.get_hash();

    let copy = entry
        .get_content()
        .join(entry.get_parents())
        .map_err(Into::into)
        .and_then(move |(content, parents)| {
            let bytes = match content {
                Content::File(blob) |
                Content::Executable(blob) => {
                    blob.into_inner()
                        .ok_or("missing blob data".into())
                        .map(Bytes::from)
                }
                Content::Symlink(path) => Ok(path.as_ref().to_vec().into()),
                Content::Tree(_) => panic!("need tree blob"),
            };

            bytes.into_future().and_then(move |bytes| {
                let nodeblob = NodeBlob {
                    parents: parents,
                    blob: hash::Sha1::from(bytes.as_ref()),
                };
                let nodekey = format!("node:{}.bincode", hash);
                let blobkey = format!("sha1:{}", nodeblob.blob);
                let nodeblob = bincode::serialize(&nodeblob, bincode::Bounded(4096))
                    .expect("bincode serialize failed");

                // TODO: blobstore.putv?
                let node = blobstore
                    .put(nodekey, Bytes::from(nodeblob))
                    .map_err(Into::into);
                let blob = blobstore.put(blobkey, bytes).map_err(Into::into);

                node.join(blob).map(|_| ())
            })
        });

    copy.boxed()
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
    revlog: RevlogRepo,
    blobstore: BBlobstore,
    csid: NodeHash,
) -> BoxFuture<BoxStream<Box<Entry<Error = mercurial::Error>>, Error>, Error> {
    let put = {
        let blobstore = blobstore.clone();
        let csid = csid;

        revlog.get_changeset_by_nodeid(&csid).from_err().and_then(
            move |cs| {
                let bcs = BlobChangeset::new(&csid, cs);

                bcs.save(blobstore).map_err(Into::into)
            },
        )
    };

    let manifest = {
        revlog
            .get_changeset_by_nodeid(&csid)
            .from_err()
            .and_then(move |cs| {
                let mfid = *cs.manifestid();

                // fetch the blob for the (root) manifest
                revlog
                    .get_manifest_blob_by_nodeid(&mfid)
                    .from_err()
                    .and_then(move |blob| {
                        let putmf = blobstore
                            .put(
                                format!("manifest:{}", mfid),
                                blob.as_blob().as_slice().expect("missing blob").into(),
                            )
                            .map_err(Into::into);

                        // Get the listing of entries and fetch each of those
                        let files = RevlogManifest::new(revlog.clone(), blob)
                            .map_err(|err| {
                                Error::with_chain(Error::from(err), "Parsing manifest to get list")
                            })
                            .map(|mf| mf.list().map_err(Error::from))
                            .into_future();

                        putmf.join(files)
                    })
            })
            .map(|(_, fs)| fs)
            .map_err(move |err| {
                Error::with_chain(err, format!("Can't copy manifest for cs {}", csid))
            })
    };

    put.join(manifest).map(|(_, fs)| fs.boxed()).boxed()
}

fn convert<H>(
    revlog: RevlogRepo,
    blobstore: BBlobstore,
    headstore: H,
    cpupool: Arc<CpuPool>,
    logger: &Logger,
) -> Result<()>
where
    H: Heads<Key = String>,
    H::Error: Into<Error>,
{

    let mut core = tokio_core::reactor::Core::new()?;

    // Generate stream of changesets. For each changeset, save the cs blob, and the manifest blob,
    // and the files. We get the set of all files as a separate flat stream, and check each one
    // against a set of seen files
    let mut seen = HashSet::new();
    let changesets = revlog.changesets()
        .map_err(Error::from)
        .enumerate()
        .map({
            let blobstore = blobstore.clone();
            let revlog = revlog.clone();
            move |(seq, csid)| {
                info!(logger, "{}: changeset {}", seq, csid);
                copy_changeset(revlog.clone(), blobstore.clone(), csid)
            }
        }) // Stream<Future<Stream<Entry>>>
        .map(Future::flatten_stream) // Stream<Stream<Entry>>
        .flatten() // Stream<Entry>
        .filter(move |entry| { // This is FnMut, with HashSet moved into its closure
            let key = (entry.get_type(), entry.get_path().clone(), *entry.get_hash());
            seen.insert(key)
        })
        .map({
            let blobstore = blobstore.clone();
            move |entry| copy_file(&entry, blobstore.clone())
        })
        .map(|copy| cpupool.spawn(copy))
        .buffer_unordered(100);

    let heads = revlog
        .get_heads()
        .map_err(Error::from)
        .map_err(|err| Error::with_chain(err, "Failed get heads"))
        .map(|h| {
            info!(logger, "head {}", h);
            headstore
                .add(&format!("{}", h))
                .map_err(Into::into)
                .map_err({
                    move |err| Error::with_chain(err, format!("Failed to create head {}", h))
                })
        })
        .buffer_unordered(100);

    let convert = changesets.merge(heads).for_each(|_| Ok(()));

    core.run(convert)?;

    Ok(())
}

fn run<In, Out>(input: In, output: Out, blobtype: BlobstoreType, logger: &Logger) -> Result<()>
where
    In: AsRef<Path>,
    Out: AsRef<Path>,
{
    let cpupool = Arc::new(CpuPool::new_num_cpus());

    let repo = open_repo(&input)?;
    let blobstore = open_blobstore(&output, blobtype)?;
    let headstore = open_headstore(&output, &cpupool)?;

    convert(repo, blobstore, headstore, cpupool, logger)
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

fn open_blobstore<P: AsRef<Path>>(output: P, ty: BlobstoreType) -> Result<BBlobstore> {
    let mut output = PathBuf::from(output.as_ref());
    output.push("blobs");

    let blobstore = match ty {
        BlobstoreType::Files => {
            Fileblob::<_, Bytes>::create(output)
                .map_err(Error::from)
                .chain_err::<_, Error>(|| "Failed to open file blob store".into())?
                .arced()
        }
        BlobstoreType::Rocksdb => {
            Rocksblob::create(output)
                .map_err(Error::from)
                .chain_err::<_, Error>(|| "Failed to open rocksdb blob store".into())?
                .arced()
        }
    };

    _assert_clone(&blobstore);
    _assert_send(&blobstore);
    _assert_static(&blobstore);
    _assert_blobstore(&blobstore);

    Ok(blobstore)
}

fn main() {
    let matches = App::new("revlog to blob importer")
        .version("0.0.0")
        .about("make blobs")
        .args_from_usage("[debug] -d, --debug     'print debug level output'")
        .arg(
            Arg::with_name("blobstore")
                .long("blobstore")
                .short("B")
                .takes_value(true)
                .possible_values(&["files", "rocksdb"])
                .required(true)
                .help("blobstore type"),
        )
        .args_from_usage("<INPUT>                 'input revlog repo'")
        .args_from_usage("<OUTPUT>                'output blobstore RepoCtx'")
        .get_matches();

    let level = if matches.is_present("debug") {
        Level::Debug
    } else {
        Level::Info
    };

    // TODO: switch to TermDecorator, which supports color
    let drain = slog_term::PlainSyncDecorator::new(io::stdout());
    let drain = slog_term::FullFormat::new(drain).build();
    let drain = LevelFilter::new(drain, level).fuse();
    let root_log = slog::Logger::root(drain, o![]);

    let input = matches.value_of("INPUT").unwrap();
    let output = matches.value_of("OUTPUT").unwrap();

    let blobtype = match matches.value_of("blobstore").unwrap() {
        "files" => BlobstoreType::Files,
        "rocksdb" => BlobstoreType::Rocksdb,
        bad => panic!("unexpected blobstore type {}", bad),
    };

    if let Err(ref e) = run(input, output, blobtype, &root_log) {
        error!(root_log, "Failed: {}", e);

        for e in e.iter().skip(1) {
            error!(root_log, "caused by: {}", e);
        }

        std::process::exit(1);
    }
}
