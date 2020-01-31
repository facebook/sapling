/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures::{future, Future};
use futures_ext::{FutureExt, StreamExt};
use manifest::{Entry, ManifestOps};
use mercurial_types::manifest::Content;
use mercurial_types::HgChangesetId;
use mononoke_types::MPath;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("{0} not found")]
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
            move |changeset| {
                changeset
                    .manifestid()
                    .find_entry(ctx, repo.get_blobstore(), path.clone())
                    .and_then(move |entry| {
                        entry.ok_or_else(|| {
                            ErrorKind::NotFound(MPath::display_opt(path.as_ref()).to_string())
                                .into()
                        })
                    })
            }
        })
        .and_then(move |entry| match entry {
            Entry::Tree(manifest_id) => manifest_id
                .load(ctx.clone(), repo.blobstore())
                .from_err()
                .map(|manifest| Content::Tree(Box::new(manifest)))
                .left_future(),
            Entry::Leaf((file_type, filenode_id)) => {
                let stream = repo.get_file_content(ctx, filenode_id).boxify();
                let content = Content::new_file(file_type, stream);
                future::ok(content).right_future()
            }
        })
}
