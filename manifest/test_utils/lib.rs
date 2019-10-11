/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobrepo::BlobRepo;
use blobstore::Loadable;
use context::CoreContext;
use failure_ext::Error;
use futures::{future, stream, Future, Stream};
use futures_ext::{bounded_traversal::bounded_traversal_stream, FutureExt};
use manifest::{Entry, Manifest};
use mercurial_types::HgChangesetId;
use mononoke_types::{BonsaiChangeset, ChangesetId, MPath};
use std::str::FromStr;
use tokio::runtime::Runtime;

pub fn get_bonsai_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    runtime: &mut Runtime,
    s: &str,
) -> (ChangesetId, BonsaiChangeset) {
    let hg_cs_id = HgChangesetId::from_str(s).unwrap();

    let bcs_id = runtime
        .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))
        .unwrap()
        .unwrap();
    let bcs = runtime
        .block_on(repo.get_bonsai_changeset(ctx.clone(), bcs_id))
        .unwrap();
    (bcs_id, bcs)
}

pub fn iterate_all_entries<MfId, LId>(
    ctx: CoreContext,
    repo: BlobRepo,
    entry: Entry<MfId, LId>,
) -> impl Stream<Item = (Option<MPath>, Entry<MfId, LId>), Error = Error>
where
    MfId: Loadable + Send + Clone,
    LId: Send + Clone + 'static,
    <MfId as Loadable>::Value: Manifest<TreeId = MfId, LeafId = LId>,
{
    let blobstore = repo.get_blobstore().clone();
    bounded_traversal_stream(256, Some((None, entry)), move |(path, entry)| match entry {
        Entry::Leaf(_) => future::ok((vec![(path, entry.clone())], vec![])).left_future(),
        Entry::Tree(tree) => tree
            .load(ctx.clone(), &blobstore)
            .map(move |mf| {
                let recurse = mf
                    .list()
                    .map(|(basename, new_entry)| {
                        let path = MPath::join_opt_element(path.as_ref(), &basename);
                        (Some(path), new_entry.clone())
                    })
                    .collect();

                (vec![(path, Entry::Tree(tree))], recurse)
            })
            .right_future(),
    })
    .map(|entries| stream::iter_ok(entries))
    .flatten()
}
