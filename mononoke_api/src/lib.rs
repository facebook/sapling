// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

pub mod errors;

use failure::Error;
use futures::Future;

use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use mercurial_types::manifest::Content;
use mercurial_types::{Changeset, HgChangesetId};
use mononoke_types::MPath;

use crate::errors::ErrorKind;

pub fn get_content_by_path(
    ctx: CoreContext,
    repo: BlobRepo,
    changesetid: HgChangesetId,
    path: Option<MPath>,
) -> impl Future<Item = Content, Error = Error> {
    repo.get_changeset_by_changesetid(ctx.clone(), changesetid)
        .from_err()
        .and_then({
            cloned!(ctx, path);
            move |changeset| repo.find_path_in_manifest(ctx, path, changeset.manifestid())
        })
        .and_then(|content| {
            content
                .ok_or_else(move || {
                    ErrorKind::NotFound(path.map(|p| p.to_string()).unwrap_or("/".to_string()))
                        .into()
                })
                .map(|(content, _)| content)
        })
}

pub fn get_changeset_by_bookmark(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark: BookmarkName,
) -> impl Future<Item = HgChangesetId, Error = Error> {
    repo.get_bookmark(ctx, &bookmark)
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
