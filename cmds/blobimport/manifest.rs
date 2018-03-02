// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::mpsc::SyncSender;

use bincode;
use bytes::Bytes;
use failure::{self, Error};
use futures::{self, Future, IntoFuture, Stream};

use blobrepo::RawNodeBlob;
use futures_ext::StreamExt;
use mercurial::RevlogRepo;
use mercurial::revlog::RevIdx;
use mercurial_types::{self, Blob, BlobHash, Entry, MPath, NodeHash, Parents, RepoPath, Type};

use BlobstoreEntry;

pub(crate) fn put_entry(
    sender: SyncSender<BlobstoreEntry>,
    entry_hash: NodeHash,
    blob: Blob,
    parents: Parents,
) -> impl Future<Item = (), Error = Error> + Send + 'static
where
    Error: Send + 'static,
{
    let bytes = blob.into_inner()
        .ok_or(failure::err_msg("missing blob data"))
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
        let nodeblob = bincode::serialize(&nodeblob)
            .expect("bincode serialize failed");

        let res1 = sender.send(BlobstoreEntry::ManifestEntry((
            nodekey,
            Bytes::from(nodeblob),
        )));
        let res2 = sender.send(BlobstoreEntry::ManifestEntry((blobkey, bytes)));

        res1.and(res2).map_err(Error::from)
    })
}

// Copy a single manifest entry into the blobstore
// TODO: #[async]
pub(crate) fn copy_entry(
    entry: Box<Entry>,
    sender: SyncSender<BlobstoreEntry>,
) -> impl Future<Item = (), Error = Error> + Send + 'static {
    let hash = (*entry).get_hash().into_nodehash();

    let blobfuture = entry.get_raw_content().map_err(Error::from);

    blobfuture
        .join(entry.get_parents().map_err(Error::from))
        .and_then(move |(blob, parents)| put_entry(sender, hash, blob, parents))
}

pub(crate) fn get_entry_stream(
    entry: Box<Entry>,
    revlog_repo: RevlogRepo,
    cs_rev: RevIdx,
    basepath: MPath,
) -> Box<Stream<Item = (Box<Entry>, RepoPath), Error = Error> + Send> {
    let path = basepath.join_element(&entry.get_name());
    let repopath = if entry.get_type() == Type::Tree {
        RepoPath::DirectoryPath(path.clone())
    } else {
        RepoPath::FilePath(path.clone())
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
        Type::File | Type::Executable | Type::Symlink => {
            futures::stream::once(Ok((entry, repopath))).boxify()
        }
        Type::Tree => entry
            .get_content()
            .and_then(|content| match content {
                mercurial_types::manifest::Content::Tree(manifest) => Ok(manifest.list()),
                _ => panic!("should not happened"),
            })
            .flatten_stream()
            .map(move |entry| {
                get_entry_stream(entry, revlog_repo.clone(), cs_rev.clone(), path.clone())
            })
            .map_err(Error::from)
            .flatten()
            .chain(futures::stream::once(Ok((entry, repopath))))
            .boxify(),
    }
}
