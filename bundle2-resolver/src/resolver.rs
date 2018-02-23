// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use bytes::Bytes;
use futures::{Future, Stream};
use futures::future::{err, ok};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use slog::Logger;

use blobrepo::BlobRepo;
use mercurial::changeset::RevlogChangeset;
use mercurial_bundles::{parts, Bundle2EncodeBuilder, Bundle2Item};
use mercurial_types::NodeHash;

use changegroup::{convert_to_revlog_changesets, convert_to_revlog_filelog, split_changegroup,
                  Filelog};
use errors::*;
use upload_blobs::{upload_blobs, UploadableBlob};
use wirepackparser::{TreemanifestBundle2Parser, TreemanifestEntry};

type PartId = u32;
type Changesets = Vec<(NodeHash, RevlogChangeset)>;
type Filelogs = HashMap<NodeHash, <Filelog as UploadableBlob>::Value>;
type Manifests = HashMap<NodeHash, <TreemanifestEntry as UploadableBlob>::Value>;

/// The resolve function takes a bundle2, interprets it's content as Changesets, Filelogs and
/// Manifests and uploades all of them to the provided BlobRepo in the correct order.
/// It returns a Future that contains the response that should be send back to the requester.
pub fn resolve(
    repo: Arc<BlobRepo>,
    logger: Logger,
    heads: Vec<String>,
    bundle2: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<Bytes, Error> {
    info!(logger, "unbundle heads {:?}", heads);

    let resolver = Bundle2Resolver::new(repo, logger);

    let bundle2 = resolver.resolve_start_and_replycaps(bundle2);

    resolver
        .resolve_changegroup(bundle2)
        .and_then(move |(changegroup_id, changesets, filelogs, bundle2)| {
            let bundle2 = resolver
                .resolve_b2xtreegroup2(bundle2)
                .and_then({
                    let resolver = resolver.clone();

                    move |(manifests, bundle2)| {
                        resolver
                            .upload_changesets(changesets, filelogs, manifests)
                            .map(|()| bundle2)
                    }
                })
                .flatten_stream()
                .boxify();

            resolver
                .ensure_stream_finished(bundle2)
                .and_then(move |()| resolver.prepare_response(changegroup_id))
        })
        .boxify()
}

fn next_item(
    bundle2: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<(Option<Bundle2Item>, BoxStream<Bundle2Item, Error>), Error> {
    bundle2.into_future().map_err(|(err, _)| err).boxify()
}

/// Holds repo and logger for convienience access from it's methods
#[derive(Clone)]
struct Bundle2Resolver {
    repo: Arc<BlobRepo>,
    logger: Logger,
}

impl Bundle2Resolver {
    fn new(repo: Arc<BlobRepo>, logger: Logger) -> Self {
        Self { repo, logger }
    }

    /// Parse Start and Replycaps and ignore their content
    fn resolve_start_and_replycaps(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxStream<Bundle2Item, Error> {
        next_item(bundle2)
            .and_then(|(start, bundle2)| match start {
                Some(Bundle2Item::Start(_)) => next_item(bundle2),
                _ => err(format_err!("Expected Bundle2 Start")).boxify(),
            })
            .and_then(|(replycaps, bundle2)| match replycaps {
                Some(Bundle2Item::Replycaps(_, part)) => part.map(|_| bundle2).boxify(),
                _ => err(format_err!("Expected Bundle2 Replycaps")).boxify(),
            })
            .flatten_stream()
            .boxify()
    }

    /// Parse changegroup.
    /// The ChangegroupId will be used in the last step for preparing response
    /// The Changesets should be parsed as RevlogChangesets and used for uploading changesets
    /// The Filelogs should be scheduled for uploading to BlobRepo and the Future resolving in
    /// their upload should be used for uploading changesets
    fn resolve_changegroup(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(PartId, Changesets, Filelogs, BoxStream<Bundle2Item, Error>), Error> {
        let repo = self.repo.clone();

        next_item(bundle2)
            .and_then(move |(changegroup, bundle2)| match changegroup {
                Some(Bundle2Item::Changegroup(header, parts)) => {
                    let part_id = header.part_id();
                    let (c, f) = split_changegroup(parts);
                    convert_to_revlog_changesets(c)
                        .collect()
                        .join(upload_blobs(repo, convert_to_revlog_filelog(f)))
                        .map(move |(changesets, filelogs)| (part_id, changesets, filelogs, bundle2))
                        .boxify()
                }
                _ => err(format_err!("Expected Bundle2 Changegroup")).boxify(),
            })
            .boxify()
    }

    /// Parse b2xtreegroup2.
    /// The Manifests should be scheduled for uploading to BlobRepo and the Future resolving in
    /// their upload as well as their parsed content should be used for uploading changesets.
    fn resolve_b2xtreegroup2(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(Manifests, BoxStream<Bundle2Item, Error>), Error> {
        let repo = self.repo.clone();

        next_item(bundle2)
            .and_then(move |(b2xtreegroup2, bundle2)| match b2xtreegroup2 {
                Some(Bundle2Item::B2xTreegroup2(_, parts)) => {
                    upload_blobs(repo, TreemanifestBundle2Parser::new(parts))
                        .map(move |manifests| (manifests, bundle2))
                        .boxify()
                }
                _ => err(format_err!("Expected Bundle2 B2xTreegroup2")).boxify(),
            })
            .boxify()
    }

    fn upload_changesets(
        &self,
        changesets: Changesets,
        filelogs: Filelogs,
        manifests: Manifests,
    ) -> BoxFuture<(), Error> {
        debug!(self.logger, "changesets: {:?}", changesets);
        debug!(self.logger, "filelogs: {:?}", filelogs.keys());
        debug!(self.logger, "manifests: {:?}", manifests.keys());

        ok(()).boxify()
    }

    /// Ensures that the next item in stream is None
    fn ensure_stream_finished(
        &self,
        bundle2: BoxStream<Bundle2Item, Error>,
    ) -> BoxFuture<(), Error> {
        next_item(bundle2)
            .and_then(|(none, _)| {
                ensure_msg!(none.is_none(), "Expected end of Bundle2");
                Ok(())
            })
            .boxify()
    }

    /// Takes a changegroup id and prepares a Bytes response containing Bundle2 with reply to
    /// changegroup part saying that the push was successful
    fn prepare_response(&self, changegroup_id: PartId) -> BoxFuture<Bytes, Error> {
        let writer = Cursor::new(Vec::new());
        let mut bundle = Bundle2EncodeBuilder::new(writer);
        // Mercurial currently hangs while trying to read compressed bundles over the wire:
        // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
        // TODO: possibly enable compression support once this is fixed.
        bundle.set_compressor_type(None);
        bundle.add_part(try_boxfuture!(parts::replychangegroup_part(
            parts::ChangegroupApplyResult::Success { heads_num_diff: 0 },
            changegroup_id,
        )));
        bundle
            .build()
            .map(|cursor| Bytes::from(cursor.into_inner()))
            .boxify()
    }
}
