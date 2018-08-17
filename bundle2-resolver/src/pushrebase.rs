// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use errors::*;
use failure::err_msg;
use futures::Future;
use futures::future::err;
use mercurial_types::HgChangesetId;
use std::sync::Arc;

pub fn do_pushrebase(
    _repo: Arc<BlobRepo>,
    _onto_bookmark: Bookmark,
    _changesets: Vec<HgChangesetId>,
) -> impl Future<Item = (), Error = Error> {
    err(err_msg("not implementd"))
}
