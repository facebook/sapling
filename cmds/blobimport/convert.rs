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
use futures_ext::{FutureExt, StreamExt};
use heads::Heads;
use mercurial::{RevlogManifest, RevlogRepo};
use mercurial::revlog::RevIdx;
use mercurial_types::{Changeset, Manifest, NodeHash};
use stats::Timeseries;

use BlobstoreEntry;
use STATS;
use errors::*;
use manifest;

pub(crate) struct ConvertContext<H> {
    pub repo: RevlogRepo,
    pub sender: SyncSender<BlobstoreEntry>,
    pub headstore: H,
    pub core: Core,
    pub cpupool: Arc<CpuPool>,
    pub logger: Logger,
}

impl<H> ConvertContext<H>
where
    H: Heads<Key = String>,
    H::Error: Into<Error>,
{
    pub fn convert(self) -> Result<()> {
        let mut core = self.core;
        let logger_owned = self.logger;
        let logger = &logger_owned;
        let cpupool = self.cpupool;
        let headstore = self.headstore;

        // Generate stream of changesets. For each changeset, save the cs blob, and the manifest
        // blob, and the files.
        let changesets = self.repo.changesets()
            .map_err(Error::from)
            .enumerate()
            .map({
                let repo = self.repo.clone();
                let sender = self.sender.clone();
                move |(seq, csid)| {
                    debug!(logger, "{}: changeset {}", seq, csid);
                    STATS::changesets.add_value(1);
                    copy_changeset(repo.clone(), sender.clone(), csid)
                }
            }) // Stream<Future<()>>
            .map(|copy| cpupool.spawn(copy))
            .buffer_unordered(100);

        let heads = self.repo
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
fn copy_changeset(
    revlog_repo: RevlogRepo,
    sender: SyncSender<BlobstoreEntry>,
    csid: NodeHash,
) -> impl Future<Item = (), Error = Error> + Send + 'static
where
    Error: Send + 'static,
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
                    .map_err(|e| Error::from(e.to_string()))
            })
    };

    let manifest = revlog_repo
        .get_changeset_by_nodeid(&csid)
        .join(revlog_repo.get_changelog_revlog_entry_by_nodeid(&csid))
        .from_err()
        .and_then(move |(cs, entry)| {
            let mfid = *cs.manifestid();
            let linkrev = entry.linkrev;

            put_blobs(revlog_repo, sender, mfid, linkrev)
        })
        .map_err(move |err| {
            Error::with_chain(err, format!("Can't copy manifest for cs {}", csid))
        });

    _assert_sized(&put);
    _assert_sized(&manifest);

    put.join(manifest).map(|_| ())
}

/// Copy manifest and filelog entries into the blob store.
///
/// See the help for copy_changeset for a full description.
fn put_blobs(
    revlog_repo: RevlogRepo,
    sender: SyncSender<BlobstoreEntry>,
    mfid: NodeHash,
    linkrev: RevIdx,
) -> impl Future<Item = (), Error = Error> + Send + 'static {
    revlog_repo
        .get_manifest_blob_by_nodeid(&mfid)
        .from_err()
        .and_then(move |blob| {
            let putmf = manifest::put_entry(
                sender.clone(),
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
                                manifest::get_entry_stream(
                                    entry,
                                    revlog_repo.clone(),
                                    linkrev.clone(),
                                )
                            }
                        })
                        .flatten()
                        .for_each(move |entry| manifest::copy_entry(entry, sender.clone()))
                })
                .into_future()
                .flatten();

            _assert_sized(&files);
            // Huh? No idea why this is needed to avoid an error below.
            let files = files.boxify();

            putmf.join(files).map(|_| ())
        })
}

fn _assert_sized<T: Sized>(_: &T) {}
