// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::Cursor;
use std::sync::Arc;

use bytes::Bytes;
use futures::{Future, Stream};
use futures::future::err;
use futures_ext::{BoxFuture, BoxStream, FutureExt};
use slog::Logger;

use blobrepo::BlobRepo;
use mercurial_bundles::{parts, Bundle2EncodeBuilder, Bundle2Item};

use changegroup::{convert_to_revlog_changesets, convert_to_revlog_filelog, split_changegroup};
use errors::*;
use upload_blobs::upload_blobs;
use wirepackparser::TreemanifestBundle2Parser;

fn next_item(
    bundle2: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<(Option<Bundle2Item>, BoxStream<Bundle2Item, Error>), Error> {
    bundle2.into_future().map_err(|(err, _)| err).boxify()
}

pub fn resolve(
    repo: Arc<BlobRepo>,
    logger: Logger,
    heads: Vec<String>,
    bundle2: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<Bytes, Error> {
    info!(logger, "unbundle heads {:?}", heads);
    next_item(bundle2)
        .and_then(|(start, bundle2)| match start {
            Some(Bundle2Item::Start(_)) => next_item(bundle2),
            _ => err(format_err!("Expected Bundle2 Start")).boxify(),
        })
        .and_then(|(replycaps, bundle2)| match replycaps {
            Some(Bundle2Item::Replycaps(_, part)) => part.and_then(|_| next_item(bundle2)).boxify(),
            _ => err(format_err!("Expected Bundle2 Replycaps")).boxify(),
        })
        .and_then({
            let repo = repo.clone();
            move |(changegroup, bundle2)| match changegroup {
                Some(Bundle2Item::Changegroup(header, parts)) => {
                    let part_id = header.part_id();
                    let (c, f) = split_changegroup(parts);
                    convert_to_revlog_changesets(c)
                        .collect()
                        .join(upload_blobs(repo.clone(), convert_to_revlog_filelog(f)))
                        .and_then(move |(changesets, filelogs)| {
                            next_item(bundle2).map(move |(b2xtreegroup2, bundle2)| {
                                ((part_id, changesets, filelogs), b2xtreegroup2, bundle2)
                            })
                        })
                        .boxify()
                }
                _ => err(format_err!("Expected Bundle2 Changegroup")).boxify(),
            }
        })
        .and_then({
            let repo = repo.clone();
            move |((part_id, changesets, filelogs), b2xtreegroup2, bundle2)| match b2xtreegroup2 {
                Some(Bundle2Item::B2xTreegroup2(_, parts)) => {
                    upload_blobs(repo.clone(), TreemanifestBundle2Parser::new(parts))
                        .map(move |manifests| (part_id, changesets, filelogs, manifests, bundle2))
                        .boxify()
                }
                _ => err(format_err!("Expected Bundle2 B2xTreegroup2")).boxify(),
            }
        })
        .and_then(
            move |(changegroup_part_id, changesets, filelogs, manifests, bundle2)| {
                debug!(logger, "changesets: {:?}", changesets);
                debug!(logger, "filelogs: {:?}", filelogs.keys());
                debug!(logger, "manifests: {:?}", manifests.keys());
                next_item(bundle2).map(move |(none, _)| (changegroup_part_id, none))
            },
        )
        .and_then(|(changegroup_part_id, none)| match none {
            None => {
                let writer = Cursor::new(Vec::new());
                let mut bundle = Bundle2EncodeBuilder::new(writer);
                // Mercurial currently hangs while trying to read compressed bundles over the wire:
                // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
                // TODO: possibly enable compression support once this is fixed.
                bundle.set_compressor_type(None);
                bundle.add_part(try_boxfuture!(parts::replychangegroup_part(
                    parts::ChangegroupApplyResult::Success { heads_num_diff: 0 },
                    changegroup_part_id,
                )));
                bundle
                    .build()
                    .map(|cursor| Bytes::from(cursor.into_inner()))
                    .boxify()
            }
            Some(_) => err(format_err!("Expected end of Bundle2")).boxify(),
        })
        .boxify()
}
