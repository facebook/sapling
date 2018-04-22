// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(conservative_impl_trait)]

extern crate blobrepo;
extern crate bytes;
extern crate changesets;
extern crate clap;
extern crate failure_ext;
extern crate futures;
extern crate futures_ext;
extern crate mercurial;
extern crate mercurial_types;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate tokio_core;

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use bytes::Bytes;
use clap::{App, Arg};
use failure_ext::{err_msg, Error, Fail, FutureFailureErrorExt, FutureFailureExt, ResultExt,
                  StreamFailureErrorExt};
use futures::{Future, IntoFuture};
use futures::future::{self, SharedItem};
use futures::stream::Stream;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use slog::Drain;
use slog_glog_fmt::default_drain as glog_drain;
use tokio_core::reactor::Core;

use blobrepo::{BlobRepo, ChangesetHandle, CreateChangeset, HgBlobEntry, UploadHgEntry};
use changesets::SqliteChangesets;
use mercurial::{HgChangesetId, HgManifestId, HgNodeHash, RevlogChangeset, RevlogEntry, RevlogRepo};
use mercurial_types::{HgBlob, MPath, RepoPath, RepositoryId, Type};

struct ParseChangeset {
    revlogcs: BoxFuture<SharedItem<RevlogChangeset>, Error>,
    rootmf: BoxFuture<(HgManifestId, HgBlob, Option<HgNodeHash>, Option<HgNodeHash>), Error>,
    entries: BoxStream<(Option<MPath>, RevlogEntry), Error>,
}

// Extracts all the data from revlog repo that commit API may need.
fn parse_changeset(revlog_repo: RevlogRepo, csid: HgChangesetId) -> ParseChangeset {
    let revlogcs = revlog_repo
        .get_changeset(&csid)
        .with_context(move |_| format!("While reading changeset {:?}", csid))
        .map_err(Fail::compat)
        .boxify()
        .shared();

    let rootmf = revlogcs
        .clone()
        .map_err(Error::from)
        .and_then({
            let revlog_repo = revlog_repo.clone();
            move |cs| {
                revlog_repo.get_root_manifest(cs.manifestid()).map({
                    let manifest_id = *cs.manifestid();
                    move |rootmf| (manifest_id, rootmf)
                })
            }
        })
        .with_context(move |_| format!("While reading root manifest for {:?}", csid))
        .map_err(Fail::compat)
        .boxify()
        .shared();

    let entries = revlogcs
        .clone()
        .map_err(Error::from)
        .and_then({
            let revlog_repo = revlog_repo.clone();
            move |cs| {
                let mut parents = cs.parents()
                    .into_iter()
                    .map(HgChangesetId::new)
                    .map(|csid| {
                        let revlog_repo = revlog_repo.clone();
                        revlog_repo
                            .get_changeset(&csid)
                            .and_then(move |cs| revlog_repo.get_root_manifest(cs.manifestid()))
                            .map(Some)
                            .boxify()
                    });

                let p1 = parents.next().unwrap_or(Ok(None).into_future().boxify());
                let p2 = parents.next().unwrap_or(Ok(None).into_future().boxify());

                p1.join(p2)
                    .with_context(move |_| format!("While reading parents of {:?}", csid))
                    .from_err()
            }
        })
        .join(rootmf.clone().from_err())
        .map(|((p1, p2), rootmf_shared)| {
            // The shared() combinator produces a SharedItem which can't be destructured in the
            // function signature.
            let (_, ref rootmf) = *rootmf_shared;
            mercurial::manifest::new_entry_intersection_stream(&rootmf, p1.as_ref(), p2.as_ref())
        })
        .flatten_stream()
        .with_context(move |_| format!("While reading entries for {:?}", csid))
        .from_err()
        .boxify();

    let revlogcs = revlogcs.map_err(Error::from).boxify();

    let rootmf = rootmf
        .map_err(Error::from)
        .and_then(move |rootmf_shared| {
            // The shared() combinator produces a SharedItem which can't be destructured in the
            // function signature.
            let (manifest_id, ref mf) = *rootmf_shared;
            let mut bytes = Vec::new();
            mf.generate(&mut bytes)
                .with_context(|_| format!("While generating root manifest blob for {:?}", csid))?;

            let (p1, p2) = mf.parents().get_nodes();
            Ok((
                manifest_id,
                HgBlob::from(Bytes::from(bytes)),
                p1.cloned(),
                p2.cloned(),
            ))
        })
        .boxify();

    ParseChangeset {
        revlogcs,
        rootmf,
        entries,
    }
}

fn upload_entry(
    blobrepo: &BlobRepo,
    entry: RevlogEntry,
    path: Option<MPath>,
) -> BoxFuture<(HgBlobEntry, RepoPath), Error> {
    let blobrepo = blobrepo.clone();

    let ty = entry.get_type();

    let path = MPath::join_element_opt(path.as_ref(), entry.get_name());
    let path = match path {
        // XXX this shouldn't be possible -- encode this in the type system
        None => {
            return future::err(err_msg(
                "internal error: joined root path with root manifest",
            )).boxify()
        }
        Some(path) => path,
    };
    let path = match ty {
        Type::Tree => RepoPath::DirectoryPath(path),
        Type::File(_) => RepoPath::FilePath(path),
    };

    let content = entry.get_raw_content();
    let parents = entry.get_parents();

    content
        .join(parents)
        .and_then(move |(content, parents)| {
            let (p1, p2) = parents.get_nodes();
            let upload = UploadHgEntry {
                nodeid: entry.get_hash().into_nodehash(),
                raw_content: content,
                content_type: ty,
                p1: p1.cloned(),
                p2: p2.cloned(),
                path,
                check_nodeid: true,
            };
            upload.upload(&blobrepo)
        })
        .and_then(|(_, entry)| entry)
        .boxify()
}

fn main() {
    let app = App::new("revlog to blob importer")
        .version("0.0.0")
        .about("make blobs")
        .args_from_usage(
            r#"
            <INPUT>                    'input revlog repo'
            --repo_id <repo_id>        'ID of the newly imported repo'
            --manifold-bucket [BUCKET] 'manifold bucket'
            --db-address [address]     'address of a db. Used only for manifold blobstore'
            --blobstore-cache-size [SIZE] 'size of the blobstore cache'
            --changesets-cache-size [SIZE] 'size of the changesets cache'
            [OUTPUT]                   'Blobstore output'
        "#,
        )
        .arg(
            Arg::with_name("blobstore")
                .long("blobstore")
                .takes_value(true)
                .possible_values(&["rocksdb", "manifold"])
                .required(true)
                .help("blobstore type"),
        );
    let matches = app.get_matches();
    let input = matches.value_of("INPUT").expect("input is not specified");
    let revlogrepo = RevlogRepo::open(input).expect("cannot open revlogrepo");

    let mut core = Core::new().expect("cannot create tokio core");

    let drain = glog_drain().fuse();
    let logger = slog::Logger::root(drain, o![]);

    let repo_id = RepositoryId::new(matches.value_of("repo_id").unwrap().parse().unwrap());

    let blobrepo = match matches.value_of("blobstore").unwrap() {
        "rocksdb" => {
            let output = matches.value_of("OUTPUT").expect("output is not specified");
            let output = Path::new(output)
                .canonicalize()
                .expect("Failed to read output path");

            assert!(
                output.is_dir(),
                "The path {:?} does not exist or is not a directory",
                output
            );

            for subdir in &[".hg", "blobs", "books", "heads"] {
                let subdir = output.join(subdir);
                if subdir.exists() {
                    assert!(
                        subdir.is_dir(),
                        "Failed to start Rocksdb BlobRepo: \
                         {:?} already exists and is not a directory",
                        subdir
                    );
                    let content: Vec<_> = subdir
                        .read_dir()
                        .expect("Failed to read directory content")
                        .collect();
                    assert!(
                        content.is_empty(),
                        "Failed to start Rocksdb BlobRepo: \
                         {:?} already exists and is not empty: {:?}",
                        subdir,
                        content
                    );
                } else {
                    fs::create_dir(&subdir)
                        .expect(&format!("Failed to create directory {:?}", subdir));
                }
            }

            {
                let changesets_path = output.join("changesets");
                assert!(
                    !changesets_path.exists(),
                    "Failed to start Rocksdb BlobRepo: {:?} already exists"
                );
                SqliteChangesets::create(changesets_path.to_string_lossy())
                    .expect("Failed to initialize changests sqlite database");
            }

            BlobRepo::new_rocksdb(
                logger.new(o!["BlobRepo:Rocksdb" => output.to_string_lossy().into_owned()]),
                &output,
                repo_id,
            ).expect("failed to create rocksdb blobrepo")
        }
        "manifold" => {
            let manifold_bucket = matches
                .value_of("manifold-bucket")
                .expect("manifold bucket is not specified");

            BlobRepo::new_test_manifold(
                logger.new(o!["BlobRepo:TestManifold" => manifold_bucket.to_owned()]),
                manifold_bucket,
                "new_blobimport_test",
                &core.remote(),
                repo_id,
                matches
                    .value_of("db-address")
                    .expect("--db-address is not specified"),
                matches
                    .value_of("blobstore-cache-size")
                    .map(|val| val.parse::<usize>().expect("cache size must be integer"))
                    .unwrap_or(100_000_000),
                matches
                    .value_of("changesets-cache-size")
                    .map(|val| val.parse::<usize>().expect("cache size must be integer"))
                    .unwrap_or(100_000_000),
            ).expect("failed to create manifold blobrepo")
        }
        bad => panic!("unexpected blobstore type: {}", bad),
    };

    let mut parent_changeset_handles: HashMap<HgNodeHash, ChangesetHandle> = HashMap::new();

    let csstream = revlogrepo
        .changesets()
        .and_then({
            let revlogrepo = revlogrepo.clone();
            move |csid| {
                let ParseChangeset {
                    revlogcs,
                    rootmf,
                    entries,
                } = parse_changeset(revlogrepo.clone(), HgChangesetId::new(csid));
                revlogcs.map(move |cs| (csid, cs, rootmf, entries))
            }
        })
        .map(move |(csid, cs, rootmf, entries)| {
            let rootmf = rootmf
                .and_then({
                    let blobrepo = blobrepo.clone();
                    move |(manifest_id, blob, p1, p2)| {
                        let upload = UploadHgEntry {
                            nodeid: manifest_id.into_nodehash(),
                            raw_content: blob,
                            content_type: Type::Tree,
                            p1,
                            p2,
                            path: RepoPath::root(),
                            // The root tree manifest is expected to have the wrong hash in hybrid
                            // mode. This will probably never go away for compatibility with
                            // old repositories.
                            check_nodeid: false,
                        };
                        upload.upload(&blobrepo)
                    }
                })
                .and_then(|(_, entry)| entry)
                .boxify();

            let entries = entries
                .and_then({
                    let blobrepo = blobrepo.clone();
                    move |(path, entry)| upload_entry(&blobrepo, entry, path)
                })
                .boxify();

            let (p1handle, p2handle) = {
                let mut parents = cs.parents().into_iter().map(|p| {
                    parent_changeset_handles
                        .get(&p)
                        .cloned()
                        .expect(&format!("parent {} not found for {}", p, csid))
                });

                (parents.next(), parents.next())
            };

            let create_changeset = CreateChangeset {
                p1: p1handle,
                p2: p2handle,
                root_manifest: rootmf,
                sub_entries: entries,
                user: String::from_utf8(Vec::from(cs.user()))
                    .expect(&format!("non-utf8 username for {}", csid)),
                time: cs.time().clone(),
                extra: cs.extra().clone(),
                comments: String::from_utf8(Vec::from(cs.comments()))
                    .expect(&format!("non-utf8 comments for {}", csid)),
            };
            let cshandle = create_changeset.create(&blobrepo);
            parent_changeset_handles.insert(csid, cshandle.clone());
            cshandle
                .get_completed_changeset()
                .with_context(move |_| format!("While uploading changeset: {}", csid))
        });

    core.run(csstream.for_each(|cs| {
        cs.map(|cs| {
            info!(logger, "inserted: {}", cs.get_changeset_id());
            ()
        }).map_err(|err| {
            error!(logger, "failed to blobimport: {}", err);

            for cause in err.causes() {
                info!(logger, "cause: {}", cause);
            }
            info!(logger, "root cause: {:?}", err.root_cause());

            let msg = format!("failed to blobimport: {}", err);
            err_msg(msg)
        })
    })).expect("main stream failed");
}
