// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

#[macro_use]
extern crate cloned;

extern crate blobrepo;
extern crate bookmarks;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate mercurial_types;
extern crate mononoke_types;

pub mod errors;

use std::sync::Arc;

use failure::Error;
use futures::Future;

use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use mercurial_types::{Changeset, HgChangesetId};
use mercurial_types::manifest::Content;
use mononoke_types::MPath;

use errors::ErrorKind;

pub fn get_content_by_path(
    repo: Arc<BlobRepo>,
    changesetid: HgChangesetId,
    path: Option<MPath>,
) -> impl Future<Item = Content, Error = Error> {
    repo.get_changeset_by_changesetid(&changesetid)
        .from_err()
        .map(|changeset| changeset.manifestid().clone().into_nodehash())
        .and_then({
            let path = path.clone();
            move |manifest| repo.find_path_in_manifest(path, manifest)
        })
        .and_then(|content| {
            content.ok_or_else(move || {
                ErrorKind::NotFound(path.map(|p| p.to_string()).unwrap_or("/".to_string())).into()
            })
        })
}

pub fn get_changeset_by_bookmark(
    repo: Arc<BlobRepo>,
    bookmark: Bookmark,
) -> impl Future<Item = HgChangesetId, Error = Error> {
    repo.get_bookmark(&bookmark)
        .map_err({
            cloned!(bookmark);
            move |_| ErrorKind::InvalidInput(bookmark.to_string()).into()
        })
        .and_then({
            cloned!(bookmark);
            move |node_cs_maybe| {
                node_cs_maybe.ok_or_else(move || ErrorKind::NotFound(bookmark.to_string()).into())
            }
        })
}
