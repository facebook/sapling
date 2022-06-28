/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use context::CoreContext;
use futures::FutureExt;
use futures::TryFutureExt;
use futures_old::stream;
use futures_old::Stream as StreamOld;
use mercurial_bundles::obsmarkers::MetadataEntry;
use mercurial_bundles::part_encode::PartEncodeBuilder;
use mercurial_bundles::parts;
use mercurial_derived_data::DeriveHgChangeset;
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

    if let Some(user) = ctx.metadata().unix_name() {
        metadata.push(MetadataEntry::new("user", user.clone()));
    }

    let part = parts::obsmarkers_part(hg_pushrebased_changesets, time, metadata);

    Some(part)
}

fn pushrebased_changesets_to_hg_stream(
    ctx: CoreContext,
    blobrepo: &BlobRepo,
    pushrebased_changesets: Vec<pushrebase::PushrebaseChangesetPair>,
) -> impl StreamOld<Item = (HgChangesetId, Vec<HgChangesetId>), Error = Error> {
    let blobrepo = blobrepo.clone();
    let futures = pushrebased_changesets.into_iter().map({
        move |p| {
            let blobrepo = blobrepo.clone();
            let ctx = ctx.clone();
            async move {
                let (old, new) = futures::try_join!(
                    blobrepo.derive_hg_changeset(&ctx, p.id_old),
                    blobrepo.derive_hg_changeset(&ctx, p.id_new),
                )?;
                Ok((old, vec![new]))
            }
            .boxed()
            .compat()
        }
    });

    stream::futures_unordered(futures)
}
