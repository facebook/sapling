// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use failure::err_msg;
use failure::prelude::*;
use futures::{Future, IntoFuture};
use futures::future::{self, SharedItem};
use futures::stream::{self, Stream};
use futures_cpupool::CpuPool;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use blobrepo::{BlobChangeset, BlobRepo, ChangesetHandle, CreateChangeset, HgBlobEntry,
               UploadHgEntry};
use mercurial::{manifest, HgChangesetId, HgManifestId, HgNodeHash, RevlogChangeset, RevlogEntry,
                RevlogRepo, NULL_HASH};
use mercurial_types::{HgBlob, MPath, RepoPath, Type};

struct ParseChangeset {
    revlogcs: BoxFuture<SharedItem<RevlogChangeset>, Error>,
    rootmf:
        BoxFuture<Option<(HgManifestId, HgBlob, Option<HgNodeHash>, Option<HgNodeHash>)>, Error>,
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
                if cs.manifestid().into_nodehash() == NULL_HASH {
                    future::ok(None).boxify()
                } else {
                    revlog_repo
                        .get_root_manifest(cs.manifestid())
                        .map({
                            let manifest_id = *cs.manifestid();
                            move |rootmf| Some((manifest_id, rootmf))
                        })
                        .boxify()
                }
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
                            .and_then(move |cs| {
                                if cs.manifestid().into_nodehash() == NULL_HASH {
                                    future::ok(None).boxify()
                                } else {
                                    revlog_repo
                                        .get_root_manifest(cs.manifestid())
                                        .map(Some)
                                        .boxify()
                                }
                            })
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
        .map(|((p1, p2), rootmf_shared)| match *rootmf_shared {
            None => stream::empty().boxify(),
            Some((_, ref rootmf)) => {
                manifest::new_entry_intersection_stream(&rootmf, p1.as_ref(), p2.as_ref())
            }
        })
        .flatten_stream()
        .with_context(move |_| format!("While reading entries for {:?}", csid))
        .from_err()
        .boxify();

    let revlogcs = revlogcs.map_err(Error::from).boxify();

    let rootmf = rootmf
        .map_err(Error::from)
        .and_then(move |rootmf_shared| match *rootmf_shared {
            None => Ok(None),
            Some((manifest_id, ref mf)) => {
                let mut bytes = Vec::new();
                mf.generate(&mut bytes).with_context(|_| {
                    format!("While generating root manifest blob for {:?}", csid)
                })?;

                let (p1, p2) = mf.parents().get_nodes();
                Ok(Some((
                    manifest_id,
                    HgBlob::from(Bytes::from(bytes)),
                    p1.cloned(),
                    p2.cloned(),
                )))
            }
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

pub fn upload_changesets(
    revlogrepo: RevlogRepo,
    blobrepo: Arc<BlobRepo>,
    cpupool_size: usize,
) -> BoxStream<BoxFuture<SharedItem<BlobChangeset>, Error>, Error> {
    let mut parent_changeset_handles: HashMap<HgNodeHash, ChangesetHandle> = HashMap::new();

    revlogrepo
        .changesets()
        .map({
            let revlogrepo = revlogrepo.clone();
            let blobrepo = blobrepo.clone();
            move |csid| {
                let ParseChangeset {
                    revlogcs,
                    rootmf,
                    entries,
                } = parse_changeset(revlogrepo.clone(), HgChangesetId::new(csid));

                let rootmf = rootmf.map({
                    let blobrepo = blobrepo.clone();
                    move |rootmf| {
                        match rootmf {
                            None => future::ok(None).boxify(),
                            Some((manifest_id, blob, p1, p2)) => {
                                let upload = UploadHgEntry {
                                    nodeid: manifest_id.into_nodehash(),
                                    raw_content: blob,
                                    content_type: Type::Tree,
                                    p1,
                                    p2,
                                    path: RepoPath::root(),
                                    // The root tree manifest is expected to have the wrong hash in
                                    // hybrid mode. This will probably never go away for
                                    // compatibility with old repositories.
                                    check_nodeid: false,
                                };
                                upload
                                    .upload(&blobrepo)
                                    .into_future()
                                    .and_then(|(_, entry)| entry)
                                    .map(Some)
                                    .boxify()
                            }
                        }
                    }
                });

                let entries = entries.map({
                    let blobrepo = blobrepo.clone();
                    move |(path, entry)| upload_entry(&blobrepo, entry, path)
                });

                revlogcs
                    .join3(rootmf, entries.collect())
                    .map(move |(cs, rootmf, entries)| (csid, cs, rootmf, entries))
            }
        })
        .map({
            let cpupool = CpuPool::new(cpupool_size);
            move |fut| cpupool.spawn(fut)
        })
        .buffered(100)
        .map(move |(csid, cs, rootmf, entries)| {
            let entries = stream::futures_unordered(entries).boxify();

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
                expected_nodeid: Some(csid),
                expected_files: Some(Vec::from(cs.files())),
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
                .from_err()
                .boxify()
        })
        .boxify()
}
