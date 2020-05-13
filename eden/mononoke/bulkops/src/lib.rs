/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

///! bulkops
///!
///! Utiltities for handling data in bulk.
use anyhow::{Error, Result};
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::TryFutureExt,
    stream::{self, StreamExt, TryStreamExt},
    Stream,
};

use changesets::{ChangesetEntry, Changesets, SqlChangesets};
use context::CoreContext;
use mononoke_types::RepositoryId;
use phases::SqlPhases;

// This function is not optimal since it could be made faster by doing more processing
// on XDB side, but for the puprpose of this binary it is good enough
pub fn fetch_all_public_changesets<'a>(
    ctx: &'a CoreContext,
    repo_id: RepositoryId,
    changesets: &'a SqlChangesets,
    phases: &'a SqlPhases,
) -> impl Stream<Item = Result<ChangesetEntry, Error>> + 'a {
    async move {
        let (start, stop) = changesets
            .get_changesets_ids_bounds(repo_id.clone())
            .compat()
            .await?;

        let start = start.ok_or_else(|| Error::msg("changesets table is empty"))?;
        let stop = stop.ok_or_else(|| Error::msg("changesets table is empty"))? + 1;
        let step = 65536;
        Ok(stream::iter(windows(start, stop, step)).map(Ok))
    }
    .try_flatten_stream()
    .and_then(move |(lower_bound, upper_bound)| async move {
        let ids = changesets
            .get_list_bs_cs_id_in_range_exclusive(repo_id, lower_bound, upper_bound)
            .compat()
            .try_collect()
            .await?;
        let mut entries = changesets
            .get_many(ctx.clone(), repo_id, ids)
            .compat()
            .await?;
        let cs_ids = entries.iter().map(|entry| entry.cs_id).collect::<Vec<_>>();
        let public = phases.get_public_raw(ctx, &cs_ids).await?;
        entries.retain(|entry| public.contains(&entry.cs_id));
        Ok::<_, Error>(stream::iter(entries).map(Ok))
    })
    .try_flatten()
}

fn windows(start: u64, stop: u64, step: u64) -> impl Iterator<Item = (u64, u64)> {
    (0..)
        .map(move |index| (start + index * step, start + (index + 1) * step))
        .take_while(move |(low, _high)| *low < stop)
        .map(move |(low, high)| (low, std::cmp::min(stop, high)))
}
