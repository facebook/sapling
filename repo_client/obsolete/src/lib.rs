/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use blobrepo::BlobRepo;
use context::CoreContext;
use failure_ext::{Error, Result};
use futures::{stream, Future, Stream};
use mercurial_bundles::obsmarkers::MetadataEntry;
use mercurial_bundles::{part_encode::PartEncodeBuilder, parts};
use mercurial_types::HgChangesetId;
use mononoke_types::DateTime;

pub fn pushrebased_changesets_to_obsmarkers_part(
    ctx: CoreContext,
    blobrepo: &BlobRepo,
    pushrebased_changesets: Vec<pushrebase::PushrebaseChangesetPair>,
) -> Option<Result<PartEncodeBuilder>> {
    let filtered_changesets: Vec<_> = pushrebased_changesets
        .into_iter()
        .filter(|c| c.id_old != c.id_new)
        .collect();

    if filtered_changesets.is_empty() {
        return None;
    }

    let hg_pushrebased_changesets =
        pushrebased_changesets_to_hg_stream(ctx.clone(), blobrepo, filtered_changesets);

    let time = DateTime::now();
    let mut metadata = vec![MetadataEntry::new("operation", "push")];

    if let Some(user) = ctx.user_unix_name() {
        metadata.push(MetadataEntry::new("user", user.clone()));
    }

    let part = parts::obsmarkers_part(hg_pushrebased_changesets, time, metadata);

    Some(part)
}

fn pushrebased_changesets_to_hg_stream(
    ctx: CoreContext,
    blobrepo: &BlobRepo,
    pushrebased_changesets: Vec<pushrebase::PushrebaseChangesetPair>,
) -> impl Stream<Item = (HgChangesetId, Vec<HgChangesetId>), Error = Error> {
    let futures = pushrebased_changesets.into_iter().map({
        move |p| {
            let hg_old = blobrepo.get_hg_from_bonsai_changeset(ctx.clone(), p.id_old);
            let hg_new = blobrepo.get_hg_from_bonsai_changeset(ctx.clone(), p.id_new);
            hg_old.join(hg_new).map(move |(old, new)| (old, vec![new]))
        }
    });

    stream::futures_unordered(futures)
}
