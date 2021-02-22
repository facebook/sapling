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

use bounded_traversal::bounded_traversal_stream;
use changesets::{ChangesetEntry, Changesets, SortOrder};
use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};
use phases::Phases;

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
    read_from_master: bool,
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
            read_from_master: true,
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

    pub fn with_read_from_master(self, read_from_master: bool) -> Self {
        Self {
            read_from_master,
            ..self
        }
    }

    /// Fetch the ChangesetEntry, which involves actually loading the Changesets
    pub fn fetch<'a>(
        &'a self,
        ctx: &'a CoreContext,
        d: Direction,
    ) -> impl Stream<Item = Result<ChangesetEntry, Error>> + 'a {
        let changesets = self.changesets.get_sql_changesets();
        let repo_id = self.repo_id;
        async move {
            let s = self
                .fetch_ids(ctx, d, None)
                .chunks(BLOBSTORE_CHUNK_SIZE)
                .then(move |results| {
                    future::ready(async move {
                        let ids: Vec<ChangesetId> = results
                            .into_iter()
                            .map(|r| r.map(|(id, _bounds)| id))
                            .collect::<Result<Vec<_>, Error>>()?;
                        let entries = changesets
                            .get_many(ctx.clone(), repo_id, ids.clone())
                            .await?;
                        let mut entries_map: HashMap<_, _> =
                            entries.into_iter().map(|e| (e.cs_id, e)).collect();
                        let result = ids
                            .into_iter()
                            .filter_map(move |id| entries_map.remove(&id))
                            .map(Ok);
                        Ok::<_, Error>(stream::iter(result))
                    })
                })
                // Allow concurrent entry chunk loads
                .buffered(2)
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
        let changesets = self.changesets.clone();
        let phases = self.phases.get_sql_phases();
        let repo_id = self.repo_id;
        let repo_bounds = if let Some(repo_bounds) = repo_bounds {
            future::ok(repo_bounds).left_future()
        } else {
            self.get_repo_bounds().right_future()
        };
        let step = self.step;
        let read_from_master = self.read_from_master;

        async move {
            let s = bounded_traversal_stream(
                1,
                Some(repo_bounds.await?),
                // Returns ids plus next bounds to query, if any
                move |(lower, upper): (u64, u64)| {
                    let changesets = changesets.clone();
                    async move {
                        let next = {
                            let changesets = changesets.clone();
                            async move {
                                let changesets = changesets.get_sql_changesets();
                                let results: Vec<_> = changesets
                                    .get_list_bs_cs_id_in_range_exclusive_limit(
                                        repo_id,
                                        lower,
                                        upper,
                                        step,
                                        d.sort_order(),
                                        read_from_master,
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
                        .boxed();
                        let handle = tokio::task::spawn(next);
                        handle.await?
                    }
                },
            )
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
        let (start, stop) = changesets
            .get_changesets_ids_bounds(self.repo_id, self.read_from_master)
            .await?;
        let start = start.ok_or_else(|| Error::msg("changesets table is empty"))?;
        let stop = stop.ok_or_else(|| Error::msg("changesets table is empty"))? + 1;
        Ok((start, stop))
    }
}

// Blobstore gets don't need batching as much as the SQL queries
const BLOBSTORE_CHUNK_SIZE: usize = 1000;

pub const MAX_FETCH_STEP: u64 = 65536;
pub const MIN_FETCH_STEP: u64 = 1;

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

    #[fbinit::test]
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

    #[fbinit::test]
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
