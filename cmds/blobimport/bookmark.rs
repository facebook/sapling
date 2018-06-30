// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use ascii::AsciiString;
use failure::prelude::*;
use futures::prelude::*;
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;

use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use mercurial::RevlogRepo;

pub fn upload_bookmarks(
    logger: &Logger,
    revlogrepo: RevlogRepo,
    blobrepo: Arc<BlobRepo>,
) -> BoxFuture<(), Error> {
    let logger = logger.clone();
    let bookmarks = Arc::new(try_boxfuture!(revlogrepo.get_bookmarks()));

    (*bookmarks).keys().map({
        let bookmarks = bookmarks.clone();
        let blobrepo = blobrepo.clone();
        move |key| {
            let blobrepo = blobrepo.clone();
            (*bookmarks).get(&key).and_then(move |v| {
                v.ok_or_else(|| format_err!("Bookmark value missing: {:?}", key))
                    .into_future()
                    .and_then(move |(cs_id, _)| {
                        blobrepo.changeset_exists(&cs_id)
                            .map(move |exists| (key, cs_id, exists))
                    })
            })
        }
    })
    .buffer_unordered(100)
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
