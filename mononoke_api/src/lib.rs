// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

pub mod errors;

use failure::Error;
use futures::{future, Future};
use futures_ext::FutureExt;

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
        .and_then({
            cloned!(repo, ctx);
            move |changeset| match path {
                None => future::ok(changeset.manifestid().into()).left_future(),
                Some(path) => repo
                    .find_entries_in_manifest(ctx, changeset.manifestid(), vec![path.clone()])
                    .and_then(move |entries| {
                        entries
                            .get(&path)
                            .copied()
                            .ok_or_else(|| ErrorKind::NotFound(path.to_string()).into())
                    })
                    .right_future(),
            }
        })
        .and_then(move |entry_id| repo.get_content_by_entryid(ctx, entry_id))
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
