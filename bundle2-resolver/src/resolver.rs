// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::Cursor;
use std::sync::Arc;

use bytes::Bytes;
use futures::{Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt};
use slog::Logger;

use blobrepo::BlobRepo;
use mercurial_bundles::{parts, Bundle2EncodeBuilder, Bundle2Item};

use changegroup::{convert_to_revlog_changesets, convert_to_revlog_filelog, split_changegroup};
use errors::*;
use upload_blobs::upload_blobs;
use wirepackparser::TreemanifestBundle2Parser;

pub fn resolve(
    repo: Arc<BlobRepo>,
    logger: Logger,
    heads: Vec<String>,
    bundle2: BoxStream<Bundle2Item, Error>,
) -> BoxFuture<Bytes, Error> {
    info!(logger, "unbundle heads {:?}", heads);
    bundle2
        .and_then(move |item| {
            debug!(logger, "bundle2 item: {:?}", item);
            match item {
                Bundle2Item::Start(_) => Ok(None).into_future().boxify(),
                Bundle2Item::Changegroup(header, parts) => {
                    let part_id = header.part_id();
                    let (c, f) = split_changegroup(parts);
                    convert_to_revlog_changesets(c)
                        .for_each({
                            let logger = logger.clone();
                            move |p| {
                                debug!(logger, "changegroup part: {:?}", p);
                                Ok(())
                            }
                        })
                        .join(upload_blobs(repo.clone(), convert_to_revlog_filelog(f)))
                        .map({
                            let logger = logger.clone();
                            move |((), map)| {
                                debug!(logger, "filelogs uploaded: {:?}", map.keys());
                            }
                        })
                        .then(move |_| {
                            parts::replychangegroup_part(
                                parts::ChangegroupApplyResult::Success { heads_num_diff: 0 },
                                part_id,
                            )
                        })
                        .map(|res| Some(res))
                        .boxify()
                }
                Bundle2Item::B2xTreegroup2(_, parts) => {
                    upload_blobs(repo.clone(), TreemanifestBundle2Parser::new(parts))
                        .map({
                            let logger = logger.clone();
                            move |map| {
                                debug!(logger, "manifests uploaded: {:?}", map.keys());
                                None
                            }
                        })
                        .boxify()
                }
                Bundle2Item::Replycaps(_, part) => part.map({
                    let logger = logger.clone();
                    move |p| {
                        debug!(logger, "replycaps part: {:?}", p);
                        None
                    }
                }).boxify(),
            }
        })
        .filter_map(|x| x)
        .collect()
        .and_then(|parts| {
            let writer = Cursor::new(Vec::new());
            let mut bundle = Bundle2EncodeBuilder::new(writer);
            // Mercurial currently hangs while trying to read compressed bundles over the wire:
            // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
            // TODO: possibly enable compression support once this is fixed.
            bundle.set_compressor_type(None);
            for part in parts {
                bundle.add_part(part);
            }
            bundle
                .build()
                .map(|cursor| Bytes::from(cursor.into_inner()))
        })
        .boxify()
}
