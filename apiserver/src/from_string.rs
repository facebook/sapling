// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// This file should only contain functions that accept a String and returns an internal type

use std::convert::TryFrom;
use std::str::FromStr;
use std::sync::Arc;

use failure::{Error, Result, ResultExt};
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};

use api;
use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_types::MPath;

use errors::ErrorKind;

pub fn get_mpath(path: String) -> Result<MPath> {
    MPath::try_from(&*path)
        .with_context(|_| ErrorKind::InvalidInput(path))
        .map_err(From::from)
}

pub fn get_changeset_id(changesetid: String) -> Result<HgChangesetId> {
    HgChangesetId::from_str(&changesetid)
        .with_context(|_| ErrorKind::InvalidInput(changesetid))
        .map_err(From::from)
}

pub fn get_bookmark(bookmark: String) -> Result<Bookmark> {
    Bookmark::new(bookmark.clone())
        .with_context(|_| ErrorKind::InvalidInput(bookmark))
        .map_err(From::from)
}
pub fn get_nodehash(hash: &str) -> Result<HgNodeHash> {
    HgNodeHash::from_str(hash)
        .with_context(|_| ErrorKind::InvalidInput(hash.to_string()))
        .map_err(From::from)
}

// interpret a string as a bookmark and find the corresponding changeset id.
// this method doesn't consider that the string could be a node hash, so any caller
// should do that check themselves, and if it fails, then attempt to use this method.
pub fn string_to_bookmark_changeset_id(
    node_string: String,
    repo: Arc<BlobRepo>,
) -> BoxFuture<HgChangesetId, Error> {
    get_bookmark(node_string.clone())
        .into_future()
        .and_then({ move |bookmark| api::get_changeset_by_bookmark(repo, bookmark).from_err() })
        .map_err({
            cloned!(node_string);
            |_| ErrorKind::InvalidInput(node_string)
        })
        .from_err()
        .boxify()
}
