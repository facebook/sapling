// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use futures::{future, Future};
use futures_ext::FutureExt;

use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use mercurial_types::manifest::Content;
use mercurial_types::{Changeset, HgChangesetId};
use mononoke_types::MPath;

use failure::Fail;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "{} not found", _0)]
    NotFound(String),
}

// This method is deprecated.  It will eventually be replaced by something more like:
// let mononoke = Mononoke::new(...);
// mononoke.repo(repo_name).changeset(changeset_id).file(path).read();
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
