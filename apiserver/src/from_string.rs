// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// This file should only contain functions that accept a String and returns an internal type

use std::convert::TryFrom;
use std::str::FromStr;

use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};

use api;
use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use context::CoreContext;
use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_types::{hash::Sha256, MPath};

use errors::ErrorKind;

pub fn get_mpath(path: String) -> Result<MPath, ErrorKind> {
    MPath::try_from(&*path).map_err(|e| ErrorKind::InvalidInput(path, Some(e)))
}

pub fn get_changeset_id(changesetid: String) -> Result<HgChangesetId, ErrorKind> {
    HgChangesetId::from_str(&changesetid).map_err(|e| ErrorKind::InvalidInput(changesetid, Some(e)))
}

pub fn get_bookmark(bookmark: String) -> Result<Bookmark, ErrorKind> {
    Bookmark::new(bookmark.clone())
        .map_err(|e| ErrorKind::InvalidInput(bookmark.to_string(), Some(e)))
}

pub fn get_nodehash(hash: &str) -> Result<HgNodeHash, ErrorKind> {
    HgNodeHash::from_str(hash).map_err(|e| ErrorKind::InvalidInput(hash.to_string(), Some(e)))
}

// interpret a string as a bookmark and find the corresponding changeset id.
// this method doesn't consider that the string could be a node hash, so any caller
// should do that check themselves, and if it fails, then attempt to use this method.
pub fn string_to_bookmark_changeset_id(
    ctx: CoreContext,
    node_string: String,
    repo: BlobRepo,
) -> BoxFuture<HgChangesetId, ErrorKind> {
    get_bookmark(node_string.clone())
        .into_future()
        .and_then(move |bookmark| api::get_changeset_by_bookmark(ctx, repo, bookmark).from_err())
        .map_err(move |e| ErrorKind::InvalidInput(node_string.to_string(), Some(e.into())))
        .boxify()
}

pub fn get_sha256_oid(oid: String) -> Result<Sha256, ErrorKind> {
    Sha256::from_str(&oid).map_err(|e| ErrorKind::InvalidInput(oid.to_string(), Some(e.into())))
}
