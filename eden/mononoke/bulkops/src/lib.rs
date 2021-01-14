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
use std::cmp::{max, min};
use std::collections::HashMap;
use std::iter::Iterator;
use std::sync::Arc;

use anyhow::{bail, Error, Result};
use futures::{
    future::{self, FutureExt, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
    Stream,
};
use itertools::Either;

use bounded_traversal::bounded_traversal_stream;
use changesets::{ChangesetEntry, Changesets, SortOrder, SqlChangesets};
use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};
use phases::{Phases, SqlPhases};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    NewestFirst,
    OldestFirst,
}

impl Direction {
    fn sort_order(&self) -> SortOrder {
        match self {
            Direction::NewestFirst => SortOrder::Descending,
            Direction::OldestFirst => SortOrder::Ascending,
        }
    }
}

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

    pub fn with_step(self, step: u64) -> Result<Self> {
        if step > MAX_FETCH_STEP {
            bail!("Step too large {}", step);
        } else if step < MIN_FETCH_STEP {
            bail!("Step too small {}", step);
        }
        Ok(Self { step, ..self })
    }

    /// Fetch the ChangesetEntry, which involves actually loading the Changesets
    pub fn fetch<'a>(
        &'a self,
        ctx: &'a CoreContext,
        d: Direction,
    ) -> impl Stream<Item = Result<ChangesetEntry, Error>> + 'a {
        let changesets = self.changesets.get_sql_changesets();
        let phases = self.phases.get_sql_phases();
        let repo_id = self.repo_id;
        let repo_bounds = self.get_repo_bounds();
        async move {
            let repo_bounds = repo_bounds.await?;
            let s = stream::iter(windows(repo_bounds, self.step, d))
                .then(move |chunk_bounds| async move {
                    let ids =
                        public_ids_for_chunk(ctx, repo_id, changesets, phases, d, chunk_bounds)
                            .await?;
                    let entries = changesets
                        .get_many(ctx.clone(), repo_id, ids.clone())
                        .await?;
                    let mut entries_map: HashMap<_, _> =
                        entries.into_iter().map(|e| (e.cs_id, e)).collect();
                    let result: Vec<_> = ids
                        .into_iter()
                        .filter_map(|id| entries_map.remove(&id))
                        .map(Ok)
                        .collect();
                    Ok::<_, Error>(stream::iter(result))
                })
                .try_flatten();
            Ok(s)
        }
        .try_flatten_stream()
    }

    /// Fetch just the ids without attempting to load the Changesets.
    /// Each id comes with the chunk bounds it was loaded from, using rusts upper exclusive bounds convention.
    /// One can optionally specify repo bounds, or None to have it resolved for you (specifying it is useful when checkpointing)
    pub fn fetch_ids<'a>(
        &'a self,
        ctx: &'a CoreContext,
        d: Direction,
        repo_bounds: Option<(u64, u64)>,
    ) -> impl Stream<Item = Result<(ChangesetId, (u64, u64)), Error>> + 'a {
        let changesets = self.changesets.get_sql_changesets();
        let phases = self.phases.get_sql_phases();
        let repo_id = self.repo_id;
        let repo_bounds = if let Some(repo_bounds) = repo_bounds {
            future::ok(repo_bounds).left_future()
        } else {
            self.get_repo_bounds().right_future()
        };
        let step = self.step;

        async move {
            let s = bounded_traversal_stream(1, Some(repo_bounds.await?), {
                // Returns ids plus next bounds to query, if any
                move |(lower, upper): (u64, u64)| {
                    async move {
                        let results: Vec<_> = changesets
                            .get_list_bs_cs_id_in_range_exclusive_limit(
                                repo_id,
                                lower,
                                upper,
                                step,
                                d.sort_order(),
                            )
                            .try_collect()
                            .await?;

                        let count = results.len() as u64;
                        let mut max_id = lower;
                        let mut min_id = upper - 1;
                        let cs_ids: Vec<ChangesetId> = results
                            .into_iter()
                            .map(|(cs_id, id)| {
                                max_id = max(max_id, id);
                                min_id = min(min_id, id);
                                cs_id
                            })
                            .collect();

                        let (completed, new_bounds) = if d == Direction::OldestFirst {
                            ((lower, max_id + 1), (max_id + 1, upper))
                        } else {
                            ((min_id, upper), (lower, min_id))
                        };

                        let (completed, new_bounds) =
                            if count < step || new_bounds.0 == new_bounds.1 {
                                ((lower, upper), None)
                            } else if new_bounds.0 >= new_bounds.1 {
                                bail!("Logic error, bad bounds {:?}", new_bounds)
                            } else {
                                // We have more to load
                                (completed, Some(new_bounds))
                            };

                        Ok::<_, Error>(((cs_ids, completed), new_bounds))
                    }
                }
            })
            .and_then(move |(mut ids, completed_bounds)| async move {
                if !ids.is_empty() {
                    let public = phases.get_public_raw(ctx, &ids).await?;
                    ids.retain(|id| public.contains(&id));
                }
                Ok::<_, Error>(stream::iter(
                    ids.into_iter().map(move |id| Ok((id, completed_bounds))),
                ))
            })
            .try_flatten();
            Ok(s)
        }
        .try_flatten_stream()
    }

    /// Get the repo bounds as max/min observed suitable for rust ranges (hence the + 1)
    pub async fn get_repo_bounds(&self) -> Result<(u64, u64), Error> {
        let changesets = self.changesets.get_sql_changesets();
        let (start, stop) = changesets.get_changesets_ids_bounds(self.repo_id).await?;
        let start = start.ok_or_else(|| Error::msg("changesets table is empty"))?;
        let stop = stop.ok_or_else(|| Error::msg("changesets table is empty"))? + 1;
        Ok((start, stop))
    }
}

pub const MAX_FETCH_STEP: u64 = 65536;
pub const MIN_FETCH_STEP: u64 = 1;

// Gets all changeset ids in a chunk that are public
// This joins changeset ids to public changesets in memory. Doing it in SQL may or may not be faster
async fn public_ids_for_chunk<'a>(
    ctx: &'a CoreContext,
    repo_id: RepositoryId,
    changesets: &'a SqlChangesets,
    phases: &'a SqlPhases,
    d: Direction,
    chunk_bounds: (u64, u64),
) -> Result<Vec<ChangesetId>> {
    let (lower, upper) = chunk_bounds;
    let mut ids: Vec<_> = changesets
        .get_list_bs_cs_id_in_range_exclusive(repo_id, lower, upper)
        .try_collect()
        .await?;
    if ids.is_empty() {
        // Most ranges are empty for small repos
        Ok(ids)
    } else {
        if d == Direction::NewestFirst {
            ids.reverse()
        }
        let public = phases.get_public_raw(ctx, &ids).await?;
        Ok(ids.into_iter().filter(|id| public.contains(&id)).collect())
    }
}

fn windows(
    repo_bounds: (u64, u64),
    step: u64,
    direction: Direction,
) -> impl Iterator<Item = (u64, u64)> {
    let (start, stop) = repo_bounds;
    if direction == Direction::NewestFirst {
        let steps = (start..stop).rev().step_by(step as usize);
        Either::Left(steps.map(move |i| (i - min(step - 1, i - start), i + 1)))
    } else {
        let steps = (start..stop).step_by(step as usize);
        Either::Right(steps.map(move |i| (i, i + min(step, stop - i))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::str::FromStr;

    use fbinit::FacebookInit;

    use blobrepo::BlobRepo;
    use bookmarks::BookmarkName;
    use fixtures::branch_wide;
    use mononoke_types::ChangesetId;
    use phases::mark_reachable_as_public;

    #[test]
    fn test_windows() -> Result<()> {
        let by_oldest: Vec<(u64, u64)> = windows((0, 13), 5, Direction::OldestFirst).collect();
        assert_eq!(by_oldest, vec![(0, 5), (5, 10), (10, 13)]);

        let by_newest: Vec<(u64, u64)> = windows((0, 13), 5, Direction::NewestFirst).collect();
        assert_eq!(by_newest, vec![(8, 13), (3, 8), (0, 3)]);
        Ok(())
    }

    async fn get_test_repo(ctx: &CoreContext, fb: FacebookInit) -> Result<BlobRepo, Error> {
        let blobrepo = branch_wide::getrepo(fb).await;

        // our function avoids derivation so we need to explicitly do the derivation for
        // phases to have any data
        let phases = blobrepo.get_phases();
        let sql_phases = phases.get_sql_phases();
        let master = BookmarkName::new("master")?;
        let master = blobrepo
            .get_bonsai_bookmark(ctx.clone(), &master)
            .await?
            .unwrap();
        mark_reachable_as_public(&ctx, sql_phases, &[master], false).await?;

        Ok(blobrepo)
    }

    fn build_fetcher(
        step_size: u64,
        blobrepo: &BlobRepo,
    ) -> Result<PublicChangesetBulkFetch, Error> {
        PublicChangesetBulkFetch::new(
            blobrepo.get_repoid(),
            blobrepo.get_changesets_object(),
            blobrepo.get_phases(),
        )
        .with_step(step_size)
    }

    #[fbinit::compat_test]
    async fn test_fetch_all_public_changesets(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobrepo = get_test_repo(&ctx, fb).await?;

        let expected = [
            "56c0203d7a9a83f14a47a17d3a10e55b1d08feb106fd72f28275e603c6e59625",
            "624aba5e7f94c9319d949bce9f0dc87f25067f01f2ca1e41b620aff0625439c8",
            "56da5b997e27f2f9020f6ff2d87b321774369e23579bd2c4ce675efad363f4f4",
        ]
        .iter()
        .map(|hex| ChangesetId::from_str(hex))
        .collect::<Result<Vec<ChangesetId>>>()?;

        // All directions
        for d in &[Direction::OldestFirst, Direction::NewestFirst] {
            let mut expected = expected.clone();
            if d == &Direction::NewestFirst {
                expected.reverse();
            }
            // Repo bounds are 1..8. Check a range of step sizes in lieu of varying the repo bounds
            for step_size in 1..9 {
                let fetcher = build_fetcher(step_size, &blobrepo)?;
                let entries: Vec<ChangesetEntry> = fetcher.fetch(&ctx, *d).try_collect().await?;
                let public_ids: Vec<ChangesetId> = entries.into_iter().map(|e| e.cs_id).collect();
                let public_ids2: Vec<ChangesetId> = fetcher
                    .fetch_ids(&ctx, *d, None)
                    .map_ok(|(cs_id, (lower, upper))| {
                        assert_ne!(lower, upper, "step {} dir {:?}", step_size, d);
                        cs_id
                    })
                    .try_collect()
                    .await?;

                assert_eq!(public_ids, public_ids2, "step {} dir {:?}", step_size, d);
                assert_eq!(public_ids, expected, "step {} dir {:?}", step_size, d);
            }
        }
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_fetch_ids_completed_bounds(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobrepo = get_test_repo(&ctx, fb).await?;

        // Check what bounds we expect each of the returned changesets to be loaded from
        let expectations: &[(u64, Direction, &[(u64, u64)])] = &[
            // Three changesets in 8 steps, so observed bounds do not abut
            (1, Direction::OldestFirst, &[(1, 2), (3, 4), (7, 8)]),
            (2, Direction::OldestFirst, &[(1, 3), (3, 5), (7, 8)]),
            // Still not abutting as first two in first step, then last in last step
            (3, Direction::OldestFirst, &[(1, 4), (1, 4), (7, 8)]),
            // Step one less than repo bounds, now abuts
            (6, Direction::OldestFirst, &[(1, 7), (1, 7), (7, 8)]),
            // Exactly cover repo bounds in one step
            (7, Direction::OldestFirst, &[(1, 8), (1, 8), (1, 8)]),
            // Slightly bigger step than repo bounds
            (8, Direction::OldestFirst, &[(1, 8), (1, 8), (1, 8)]),
            // Three changesets in 8 steps, so observed bounds do not abut
            (1, Direction::NewestFirst, &[(7, 8), (3, 4), (1, 2)]),
            (2, Direction::NewestFirst, &[(6, 8), (2, 4), (1, 2)]),
            // In this direction starts to abut
            (3, Direction::NewestFirst, &[(5, 8), (2, 5), (1, 2)]),
            // Step one less than repo bounds, now abuts
            (6, Direction::NewestFirst, &[(2, 8), (2, 8), (1, 2)]),
            // Exactly cover repo bounds in one step
            (7, Direction::NewestFirst, &[(1, 8), (1, 8), (1, 8)]),
            // Slightly bigger step than repo bounds
            (8, Direction::NewestFirst, &[(1, 8), (1, 8), (1, 8)]),
        ];

        for (step, dir, expected_completed) in expectations.iter() {
            let fetcher = build_fetcher(*step, &blobrepo)?;

            let repo_bounds: (u64, u64) = fetcher.get_repo_bounds().await?;
            assert_eq!((1, 8), repo_bounds);

            let completed: Vec<(u64, u64)> = fetcher
                .fetch_ids(&ctx, *dir, Some(repo_bounds))
                .map_ok(|(_cs_id, completed)| completed)
                .try_collect()
                .await?;
            assert_eq!(
                completed.as_slice(),
                *expected_completed,
                "step {} dir {:?}",
                step,
                dir
            );
        }
        Ok(())
    }
}
