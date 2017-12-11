// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use futures::{Future, IntoFuture, Stream};
use futures_cpupool::CpuPool;
use slog::Logger;
use tokio_core::reactor::Core;

use blobrepo::BlobChangeset;
use failure::{Error, Result};
use futures_ext::{BoxStream, FutureExt, StreamExt};
use heads::Heads;
use linknodes::Linknodes;
use mercurial::{self, RevlogManifest, RevlogRepo};
use mercurial::revlog::RevIdx;
use mercurial_types::{Changeset, Manifest, NodeHash, RepoPath};
use stats::Timeseries;

use BlobstoreEntry;
use STATS;
use manifest;

pub(crate) struct ConvertContext<H> {
    pub repo: RevlogRepo,
    pub sender: SyncSender<BlobstoreEntry>,
    pub headstore: H,
    pub core: Core,
    pub cpupool: Arc<CpuPool>,
    pub logger: Logger,
    pub skip: Option<u64>,
    pub commits_limit: Option<u64>,
}

impl<H> ConvertContext<H>
where
    H: Heads,
{
    pub fn convert<L: Linknodes>(self, linknodes_store: L) -> Result<()> {
        let mut core = self.core;
        let logger_owned = self.logger;
        let logger = &logger_owned;
        let cpupool = self.cpupool;
        let headstore = self.headstore;
        let skip = self.skip;
        let commits_limit = self.commits_limit;

        let changesets: BoxStream<NodeHash, mercurial::Error> = if let Some(skip) = skip {
            self.repo.changesets().skip(skip).boxify()
        } else {
            self.repo.changesets().boxify()
        };

        let changesets: BoxStream<NodeHash, mercurial::Error> = if let Some(limit) = commits_limit {
            changesets.take(limit).boxify()
        } else {
            changesets.boxify()
        };
        let linknodes_store = Arc::new(linknodes_store);

        // Generate stream of changesets. For each changeset, save the cs blob, and the manifest
        // blob, and the files.
        let changesets = changesets
            .map_err(Error::from)
            .enumerate()
            .map({
                let repo = self.repo.clone();
                let sender = self.sender.clone();
                move |(seq, csid)| {
                    debug!(logger, "{}: changeset {}", seq, csid);
                    STATS::changesets.add_value(1);
                    copy_changeset(repo.clone(), sender.clone(), linknodes_store.clone(), csid)
                }
            }) // Stream<Future<()>>
            .map(|copy| cpupool.spawn(copy))
            .buffer_unordered(100);

        let heads = self.repo
            .get_heads()
            .map_err(Error::from)
            .map_err(|err| err.context("Failed get heads").into())
            .map(|h| {
                debug!(logger, "head {}", h);
                STATS::heads.add_value(1);
                headstore.add(&h).map_err({
                    move |err| {
                        err.context(format_err!("Failed to create head {}", h))
                            .into()
                    }
                })
            })
            .buffer_unordered(100);

        let convert = changesets.select(heads).for_each(|_| Ok(()));

        core.run(convert)?;

        info!(logger, "parsed everything, waiting for io");
        Ok(())
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
fn copy_changeset<L>(
    revlog_repo: RevlogRepo,
    sender: SyncSender<BlobstoreEntry>,
    linknodes_store: L,
    csid: NodeHash,
) -> impl Future<Item = (), Error = Error> + Send + 'static
where
    Error: Send + 'static,
    L: Linknodes,
{
    let put = {
        let sender = sender.clone();
        let csid = csid;

        revlog_repo
            .get_changeset_by_nodeid(&csid)
            .from_err()
            .and_then(move |cs| {
                let bcs = BlobChangeset::new(&csid, cs);
                sender
                    .send(BlobstoreEntry::Changeset(bcs))
                    .map_err(Error::from)
            })
    };

    let manifest = revlog_repo
        .get_changeset_by_nodeid(&csid)
        .join(revlog_repo.get_changelog_revlog_entry_by_nodeid(&csid))
        .from_err()
        .and_then(move |(cs, entry)| {
            let mfid = *cs.manifestid();
            let linkrev = entry.linkrev;
            put_blobs(revlog_repo, sender, linknodes_store, mfid, linkrev)
        })
        .map_err(move |err| {
            err.context(format_err!("Can't copy manifest for cs {}", csid))
                .into()
        });
    _assert_sized(&put);
    _assert_sized(&manifest);

    put.join(manifest).map(|_| ())
}

/// Copy manifest and filelog entries into the blob store.
///
/// See the help for copy_changeset for a full description.
fn put_blobs<L>(
    revlog_repo: RevlogRepo,
    sender: SyncSender<BlobstoreEntry>,
    linknodes_store: L,
    mfid: NodeHash,
    linkrev: RevIdx,
) -> impl Future<Item = (), Error = Error> + Send + 'static
where
    L: Linknodes,
{
    let cs_entry_fut = revlog_repo.get_changelog().get_entry(linkrev).into_future();

    revlog_repo
        .get_manifest_blob_by_nodeid(&mfid)
        .join(cs_entry_fut)
        .from_err()
        .and_then(move |(blob, cs_entry)| {
            let putmf = manifest::put_entry(
                sender.clone(),
                mfid,
                blob.as_blob().clone(),
                blob.parents().clone(),
            );

            let linknode = cs_entry.nodeid;
            let put_root_linknode = linknodes_store.add(RepoPath::root(), &mfid, &linknode);

            // Get the listing of entries and fetch each of those
            let files = RevlogManifest::new(revlog_repo.clone(), blob)
                .map_err(|err| {
                    Error::from(err.context("Parsing manifest to get list"))
                })
                .map(|mf| mf.list().map_err(Error::from))
                .map(|entry_stream| {
                    entry_stream
                        .map({
                            let revlog_repo = revlog_repo.clone();
                            move |entry| {
                                manifest::get_entry_stream(
                                    entry,
                                    revlog_repo.clone(),
                                    linkrev.clone(),
                                )
                            }
                        })
                        .flatten()
                        .for_each(move |entry| {
                            // All entries share the same linknode to the changelog.
                            let linknode_future = linknodes_store.add(
                                entry.get_path().clone(),
                                entry.get_hash(),
                                &linknode,
                            );
                            let copy_future = manifest::copy_entry(entry, sender.clone());
                            copy_future.join(linknode_future).map(|_| ())
                        })
                })
                .into_future()
                .flatten();

            _assert_sized(&files);
            // Huh? No idea why this is needed to avoid an error below.
            let files = files.boxify();

            putmf.join3(put_root_linknode, files).map(|_| ())
        })
}

fn _assert_sized<T: Sized>(_: &T) {}
