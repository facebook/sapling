// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(deprecated)]

use std::sync::mpsc::SyncSender;

use failure::{self, Error};
use futures::{self, stream, Future, IntoFuture, Stream};

use blobrepo::RawNodeBlob;
use futures_ext::StreamExt;
use mercurial::{self, RevlogEntry, RevlogRepo};
use mercurial::revlog::RevIdx;
use mercurial::revlogrepo::RevlogRepoBlobimportExt;
use mercurial_types::{HgBlob, HgNodeHash, HgParents, MPath, RepoPath, Type};

use BlobstoreEntry;

pub(crate) fn put_entry(
    sender: SyncSender<BlobstoreEntry>,
    entry_hash: HgNodeHash,
    blob: HgBlob,
    parents: HgParents,
) -> impl Future<Item = (), Error = Error> + Send + 'static
where
    Error: Send + 'static,
{
    let blob = blob.clean();

    let nodeblob = RawNodeBlob {
        parents,
        blob: blob.hash().expect("clean blob must have hash"),
    };
    // TODO: (jsgf) T21597565 Convert blobimport to use blobrepo methods to name and create
    // blobs.
    let nodekey = format!("node-{}.bincode", entry_hash);
    let blobkey = format!("sha1-{}", nodeblob.blob.sha1());
    let nodeblob = nodeblob.serialize(&entry_hash).expect("serialize failed");

    let res1 = sender.send(BlobstoreEntry::ManifestEntry((nodekey, nodeblob.into())));
    let res2 = sender.send(BlobstoreEntry::ManifestEntry((
        blobkey,
        // Manifests are serialized as they are in Mercurial, so just
        // uploading the exact bytes as they are in Mercurial is valid.
        blob.into(),
    )));

    res1.and(res2).map_err(Error::from).into_future()
}

// Copy a single manifest entry into the blobstore
// TODO: #[async]
pub(crate) fn copy_entry(
    entry: RevlogEntry,
    sender: SyncSender<BlobstoreEntry>,
) -> impl Future<Item = (), Error = Error> + Send + 'static {
    let hash = entry.get_hash().into_nodehash();

    let blobfuture = entry.get_raw_content().map_err(Error::from);

    blobfuture
        .join(entry.get_parents().map_err(Error::from))
        .and_then(move |(blob, parents)| put_entry(sender, hash, blob, parents))
}

pub(crate) fn get_entry_stream(
    entry: RevlogEntry,
    revlog_repo: RevlogRepo,
    cs_rev: RevIdx,
    basepath: Option<&MPath>,
) -> Box<Stream<Item = (RevlogEntry, RepoPath), Error = Error> + Send> {
    let path = MPath::join_element_opt(basepath, entry.get_name());
    let repopath = match path.as_ref() {
        None => {
            // XXX clean this up so that this assertion is encoded in the type system
            return stream::once(Err(failure::err_msg(
                "internal error: joined root path with root manifest",
            ))).boxify();
        }
        Some(path) => match entry.get_type() {
            Type::Tree => RepoPath::DirectoryPath(path.clone()),
            Type::File(_) => RepoPath::FilePath(path.clone()),
        },
    };
    let revlog = revlog_repo.get_path_revlog(&repopath);

    let linkrev = revlog
        .and_then(|file_revlog| file_revlog.get_entry_by_id(&entry.get_hash()))
        .map(|e| e.linkrev)
        .map_err(|e| {
            e.context(format_err!(
                "cannot get linkrev of {}",
                entry.get_hash().into_nodehash()
            )).into()
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
        Type::File(_) => futures::stream::once(Ok((entry, repopath))).boxify(),
        Type::Tree => entry
            .get_content()
            .and_then(|content| match content {
                mercurial::EntryContent::Tree(manifest) => Ok(manifest.list()),
                _ => panic!("should not happened"),
            })
            .flatten_stream()
            .map(move |entry| {
                get_entry_stream(entry, revlog_repo.clone(), cs_rev.clone(), path.as_ref())
            })
            .map_err(Error::from)
            .flatten()
            .chain(futures::stream::once(Ok((entry, repopath))))
            .boxify(),
    }
}
