// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::sync::Arc;

use ascii::AsciiString;
use failure::prelude::*;
use futures::{stream, prelude::*};
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;

use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use mercurial::RevlogRepo;
use mercurial_types::HgChangesetId;

pub fn read_bookmarks(revlogrepo: RevlogRepo) -> BoxFuture<Vec<(Vec<u8>, HgChangesetId)>, Error> {
    let bookmarks = Arc::new(try_boxfuture!(revlogrepo.get_bookmarks()));

    (*bookmarks)
        .keys()
        .and_then({
            let bookmarks = bookmarks.clone();
            move |key| {
                (*bookmarks).get(&key).and_then(move |cs_id| {
                    cs_id
                        .ok_or_else(|| format_err!("Bookmark value missing: {:?}", key))
                        .map(move |(cs_id, _)| (key, cs_id))
                })
            }
        })
        .collect()
        .boxify()
}

pub fn upload_bookmarks(
    logger: &Logger,
    revlogrepo: RevlogRepo,
    blobrepo: Arc<BlobRepo>,
    stale_bookmarks: Vec<(Vec<u8>, HgChangesetId)>,
) -> BoxFuture<(), Error> {
    let logger = logger.clone();
    let stale_bookmarks = Arc::new(stale_bookmarks.into_iter().collect::<HashMap<_, _>>());

    read_bookmarks(revlogrepo)
        .map({
            let logger = logger.clone();
            let blobrepo = blobrepo.clone();
            let stale_bookmarks = stale_bookmarks.clone();
            move |bookmarks| {
                stream::futures_unordered(bookmarks.into_iter().map(|(key, cs_id)| {
                    let logger = logger.clone();
                    let blobrepo = blobrepo.clone();
                    let stale_bookmarks = stale_bookmarks.clone();
                    blobrepo
                        .changeset_exists(&cs_id)
                        .and_then({
                            let logger = logger.clone();
                            let key = key.clone();
                            move |exists| {
                                match (exists, stale_bookmarks.get(&key).cloned()) {
                                    (false, Some(stale_cs_id)) => {
                                        info!(
                                            logger,
                                            "current version of bookmark {:?} couldn't be \
                                            imported, because cs {:?} was not present in blobrepo \
                                            yet; using stale version instead {:?}",
                                            key,
                                            cs_id,
                                            stale_cs_id,
                                        );

                                        blobrepo
                                            .changeset_exists(&stale_cs_id)
                                            .map(move |exists| (key, stale_cs_id, exists))
                                            .boxify()
                                    }
                                    _ => Ok((key, cs_id, exists)).into_future().boxify(),
                                }
                        }})
                }))
            }
        })
        .flatten_stream()
        .filter_map({
            let logger = logger.clone();
            move |(key, cs_id, exists)| {
                if exists {
                    Some((key, cs_id))
                } else {
                    info!(
                        logger,
                        "did not update bookmark {:?}, because cs {:?} was not imported yet",
                        key,
                        cs_id,
                    );
                    None
               }
            }
        })
        .chunks(100) // send 100 bookmarks in a single transaction
        .and_then({
            let blobrepo = blobrepo.clone();
            move |vec| {
                let count = vec.len();
                let mut transaction = blobrepo.update_bookmark_transaction();

                for (key, value) in vec {
                    let key = Bookmark::new_ascii(try_boxfuture!(AsciiString::from_ascii(key)));
                    try_boxfuture!(transaction.force_set(&key, &value))
                }

                transaction.commit()
                    .and_then(move |ok| {
                        if ok {
                            Ok(count)
                        } else {
                            Err(format_err!("Bookmark transaction failed"))
                        }
                    })
                    .boxify()
            }
        }).for_each(move |count| {
            info!(logger, "uploaded chunk of {:?} bookmarks", count);
            Ok(())
        }).boxify()
}
