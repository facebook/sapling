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
use std::cmp::min;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Error, Result};
use futures::{
    future::{try_join, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
    Stream,
};

use changesets::{ChangesetEntry, Changesets, SqlChangesets};
use context::CoreContext;
use mononoke_types::RepositoryId;
use phases::{Phases, SqlPhases};

pub struct PublicChangesetBulkFetch {
    repo_id: RepositoryId,
    changesets: Arc<dyn Changesets>,
    phases: Arc<dyn Phases>,
    step: u64,
}

impl PublicChangesetBulkFetch {
    pub fn new(
        repo_id: RepositoryId,
        changesets: Arc<dyn Changesets>,
        phases: Arc<dyn Phases>,
    ) -> Self {
        Self {
            repo_id,
            changesets,
            phases,
            step: MAX_FETCH_STEP,
        }
    }

    pub fn with_step(self, step: u64) -> Self {
        Self { step, ..self }
    }

    pub fn fetch<'a>(
        &'a self,
        ctx: &'a CoreContext,
    ) -> impl Stream<Item = Result<ChangesetEntry, Error>> + 'a {
        let changesets = self.changesets.get_sql_changesets();
        let phases = self.phases.get_sql_phases();
        fetch_all_public_changesets(ctx, self.repo_id, changesets, phases, self.step)
    }
}

pub const MAX_FETCH_STEP: u64 = 65536;
pub const MIN_FETCH_STEP: u64 = 1;

// This joins changeset ids to public changesets in memory. Doing it in SQL may or may not be faster
fn fetch_all_public_changesets<'a>(
    ctx: &'a CoreContext,
    repo_id: RepositoryId,
    changesets: &'a SqlChangesets,
    phases: &'a SqlPhases,
    step: u64,
) -> impl Stream<Item = Result<ChangesetEntry, Error>> + 'a {
    async move {
        if step > MAX_FETCH_STEP {
            bail!("Step too large {}", step);
        } else if step < MIN_FETCH_STEP {
            bail!("Step too small {}", step);
        }
        let (start, stop) = changesets
            .get_changesets_ids_bounds(repo_id.clone())
            .await?;

        let start = start.ok_or_else(|| Error::msg("changesets table is empty"))?;
        let stop = stop.ok_or_else(|| Error::msg("changesets table is empty"))? + 1;
        Ok(stream::iter(windows((start, stop), step)).map(Ok))
    }
    .try_flatten_stream()
    .and_then(move |(lower_bound, upper_bound)| async move {
        let ids: Vec<_> = changesets
            .get_list_bs_cs_id_in_range_exclusive(repo_id, lower_bound, upper_bound)
            .try_collect()
            .await?;
        let (entries, public) = try_join(
            changesets.get_many(ctx.clone(), repo_id, ids.clone()),
            phases.get_public_raw(ctx, &ids),
        )
        .await?;
        let mut entries_map: HashMap<_, _> = entries.into_iter().map(|e| (e.cs_id, e)).collect();
        let result: Vec<_> = ids
            .into_iter()
            .filter(|id| public.contains(&id))
            .filter_map(|id| entries_map.remove(&id))
            .collect();
        Ok::<_, Error>(stream::iter(result).map(Ok))
    })
    .try_flatten()
}

fn windows(repo_bounds: (u64, u64), step: u64) -> impl Iterator<Item = (u64, u64)> {
    let (start, stop) = repo_bounds;
    let steps = (start..stop).step_by(step as usize);
    steps.map(move |i| (i, i + min(step, stop - i)))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::str::FromStr;

    use fbinit::FacebookInit;

    use blobrepo::BlobRepo;
    use blobrepo_hg::BlobRepoHg;
    use bookmarks::BookmarkName;
    use fixtures::branch_wide;
    use mercurial_types::HgChangesetId;
    use phases::mark_reachable_as_public;

    #[test]
    fn test_windows() -> Result<()> {
        let by_oldest: Vec<(u64, u64)> = windows((0, 13), 5).collect();
        assert_eq!(by_oldest, vec![(0, 5), (5, 10), (10, 13)]);
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_fetch_all_public_changesets(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobrepo = branch_wide::getrepo(fb).await;

        // our function avoids derivation so we need to explicitly do the derivation for
        // phases to have any data
        {
            let phases = blobrepo.get_phases();
            let sql_phases = phases.get_sql_phases();
            let master = BookmarkName::new("master")?;
            let master = blobrepo
                .get_bonsai_bookmark(ctx.clone(), &master)
                .await?
                .unwrap();
            mark_reachable_as_public(&ctx, sql_phases, &[master], false).await?;
        }

        // Check a range of step sizes in lieu of varying the repo bounds
        for step_size in 1..5 {
            check_step_size(&ctx, &blobrepo, step_size as u64).await?
        }
        Ok(())
    }

    async fn check_step_size(ctx: &CoreContext, blobrepo: &BlobRepo, step_size: u64) -> Result<()> {
        let fetcher = PublicChangesetBulkFetch::new(
            blobrepo.get_repoid(),
            blobrepo.get_changesets_object(),
            blobrepo.get_phases(),
        )
        .with_step(step_size);

        let public_changesets: Vec<ChangesetEntry> = fetcher.fetch(&ctx).try_collect().await?;

        let mut hg_mapped = vec![];
        for cs_entry in public_changesets {
            let hg_cs_id = blobrepo
                .get_hg_from_bonsai_changeset(ctx.clone(), cs_entry.cs_id)
                .await?;
            hg_mapped.push(hg_cs_id);
        }
        let expected = vec![
            "ecba698fee57eeeef88ac3dcc3b623ede4af47bd",
            "4685e9e62e4885d477ead6964a7600c750e39b03",
            "49f53ab171171b3180e125b918bd1cf0af7e5449",
        ];
        let expected_mapped = expected
            .iter()
            .map(|v| HgChangesetId::from_str(v))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(hg_mapped, expected_mapped);
        Ok(())
    }
}
