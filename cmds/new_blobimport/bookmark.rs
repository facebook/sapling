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
use mercurial_types::DChangesetId;

pub fn upload_bookmarks(
    logger: &Logger,
    revlogrepo: RevlogRepo,
    blobrepo: Arc<BlobRepo>,
) -> BoxFuture<(), Error> {
    let logger = logger.clone();
    let bookmarks = Arc::new(try_boxfuture!(revlogrepo.get_bookmarks()));

    (*bookmarks).keys().and_then({
        let bookmarks = bookmarks.clone();
        move |key| {
            (*bookmarks).get(&key).and_then(move |v| {
                let (cs_id, _) = v.ok_or_else(|| format_err!("Bookmark value missing: {:?}", key))?;
                Ok((key, cs_id))
            })
        }
    }).chunks(100) // send 100 bookmarks in a single transaction
    .and_then({
        let blobrepo = blobrepo.clone();
        move |vec| {
            let count = vec.len();
            let mut transaction = blobrepo.update_bookmark_transaction();

            for (key, value) in vec {
                let key = Bookmark::new_ascii(try_boxfuture!(AsciiString::from_ascii(key)));
                let value = DChangesetId::new(value.into_nodehash().into_mononoke());
                try_boxfuture!(transaction.create(&key, &value))
            }

            transaction.commit().map(move |()| count).boxify()
        }
    }).for_each(move |count| {
        info!(logger, "uploaded chunk of {:?} bookmarks", count);
        Ok(())
    }).boxify()
}
