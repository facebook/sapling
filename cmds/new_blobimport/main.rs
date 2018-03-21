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
use failure_ext::{err_msg, Error, Fail, ResultExt};
use futures::{Future, IntoFuture};
use futures::future::{self, SharedItem};
use futures::stream::Stream;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use slog::Drain;
use slog_glog_fmt::default_drain as glog_drain;
use tokio_core::reactor::Core;

use blobrepo::{BlobChangeset, BlobEntry, BlobRepo, ChangesetHandle};
use changesets::SqliteChangesets;
use mercurial::RevlogRepo;
use mercurial_types::{Blob, Changeset, Entry, HgChangesetId, MPath, NodeHash, RepoPath,
                      RepositoryId, Type};
use mercurial_types::manifest_utils::new_entry_intersection_stream;

struct ParseChangeset {
    blobcs: BoxFuture<SharedItem<BlobChangeset>, Error>,
    rootmf: BoxFuture<(Blob, Option<NodeHash>, Option<NodeHash>), Error>,
    entries: BoxStream<(Option<MPath>, Box<Entry + Sync>), Error>,
}

// Extracts all the data from revlog repo that commit API may need.
fn parse_changeset(revlog_repo: RevlogRepo, csid: HgChangesetId) -> ParseChangeset {
    let blobcs = revlog_repo
        .get_changeset(&csid)
        .and_then(BlobChangeset::new)
        .map_err(move |err| err.context(format!("While reading changeset {:?}", csid)))
        .map_err(Fail::compat)
        .boxify()
        .shared();

    let rootmf = blobcs
        .clone()
        .map_err(Error::from)
        .and_then({
            let revlog_repo = revlog_repo.clone();
            move |cs| revlog_repo.get_root_manifest(cs.manifestid())
        })
        .map_err(move |err| err.context(format!("While reading root manifest for {:?}", csid)))
        .map_err(Fail::compat)
        .boxify()
        .shared();

    let entries = blobcs
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

                p1.join(p2).map_err(move |err| {
                    err.context(format!("While reading parents of {:?}", csid))
                        .into()
                })
            }
        })
        .join(rootmf.clone().from_err())
        .map(|((p1, p2), rootmf)| new_entry_intersection_stream(&*rootmf, p1.as_ref(), p2.as_ref()))
        .flatten_stream()
        .map_err(move |err| {
            Error::from(err.context(format!("While reading entries for {:?}", csid)))
        })
        .boxify();

    let blobcs = blobcs.map_err(Error::from).boxify();

    let rootmf = rootmf
        .map_err(Error::from)
        .and_then(move |mf| {
            let mut bytes = Vec::new();
            mf.generate(&mut bytes)
                .with_context(|_| format!("While generating root manifest blob for {:?}", csid))?;

            let (p1, p2) = mf.parents().get_nodes();
            Ok((Blob::from(Bytes::from(bytes)), p1.cloned(), p2.cloned()))
        })
        .boxify();

    ParseChangeset {
        blobcs,
        rootmf,
        entries,
    }
}

fn upload_entry(
    blobrepo: &BlobRepo,
    entry: Box<Entry>,
    path: Option<MPath>,
) -> BoxFuture<(BlobEntry, RepoPath), Error> {
    let blobrepo = blobrepo.clone();

    let entryid = *entry.get_hash();

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
        Type::File | Type::Symlink | Type::Executable => RepoPath::FilePath(path),
    };

    let content = entry.get_raw_content().and_then(move |content| {
        let content = content
            .as_slice()
            .ok_or_else(|| err_msg(format!("corrupt blob {}", entryid)))?;
        // convert from Blob<Vec<u8>> to Blob<Bytes>
        Ok(Blob::from(Bytes::from(content)))
    });

    let parents = entry.get_parents().map(|parents| {
        let (p1, p2) = parents.get_nodes();
        (p1.cloned(), p2.cloned())
    });

    content
        .join(parents)
        .and_then(move |(content, parents)| {
            let (p1, p2) = parents;
            blobrepo.upload_entry(content, ty, p1, p2, path)
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

            for subdir in &[".hg", "blobs", "books", "heads", "linknodes"] {
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
            ).expect("failed to create manifold blobrepo")
        }
        bad => panic!("unexpected blobstore type: {}", bad),
    };

    let mut parent_changeset_handles: HashMap<NodeHash, ChangesetHandle> = HashMap::new();

    let csstream = revlogrepo
        .changesets()
        .and_then({
            let revlogrepo = revlogrepo.clone();
            move |node| {
                let ParseChangeset {
                    blobcs,
                    rootmf,
                    entries,
                } = parse_changeset(revlogrepo.clone(), HgChangesetId::new(node));
                blobcs.map(move |cs| (cs, rootmf, entries))
            }
        })
        .map(move |(cs, rootmf, entries)| {
            let csid = cs.get_changeset_id();

            let rootmf = rootmf
                .and_then({
                    let blobrepo = blobrepo.clone();
                    move |(blob, p1, p2)| {
                        blobrepo.upload_entry(blob, Type::Tree, p1, p2, RepoPath::root())
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
                        .expect(&format!("parent {} not found for {:?}", p, csid))
                });

                (parents.next(), parents.next())
            };

            let cshandle = blobrepo.create_changeset(
                p1handle,
                p2handle,
                rootmf,
                entries,
                String::from_utf8(Vec::from(cs.user()))
                    .expect(&format!("non-utf8 username for {:?}", csid)),
                cs.time().clone(),
                cs.extra().clone(),
                String::from_utf8(Vec::from(cs.comments()))
                    .expect(&format!("non-utf8 comments for {:?}", csid)),
            );
            parent_changeset_handles
                .insert(cs.get_changeset_id().into_nodehash(), cshandle.clone());
            cshandle.get_completed_changeset().map_err({
                let csid = cs.get_changeset_id();
                move |e| Error::from(e.context(format!("While uploading changeset: {:?}", csid)))
            })
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
