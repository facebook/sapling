/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

///! bulkops
///!
///! Utiltities for handling data in bulk.
use std::cmp::max;
///! bulkops
///!
///! Utiltities for handling data in bulk.
use std::cmp::min;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use bounded_traversal::bounded_traversal_stream;
use changesets::ChangesetEntry;
use changesets::Changesets;
use changesets::SortOrder;
use context::CoreContext;
use futures::future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Stream;
use mononoke_types::ChangesetId;
use phases::Phases;
use strum_macros::AsRefStr;
use strum_macros::EnumString;
use strum_macros::EnumVariantNames;

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    AsRefStr,
    EnumVariantNames,
    EnumString
)]
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
    changesets: Arc<dyn Changesets>,
    phases: Arc<dyn Phases>,
    read_from_master: bool,
    step: u64,
}

impl PublicChangesetBulkFetch {
    pub fn new(changesets: Arc<dyn Changesets>, phases: Arc<dyn Phases>) -> Self {
        Self {
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
        self.fetch_bounded(ctx, d, None)
    }

    pub fn fetch_bounded<'a>(
        &'a self,
        ctx: &'a CoreContext,
        d: Direction,
        repo_bounds: Option<(u64, u64)>,
    ) -> impl Stream<Item = Result<ChangesetEntry, Error>> + 'a {
        async move {
            let s = self
                .fetch_ids(ctx, d, repo_bounds)
                .chunks(BLOBSTORE_CHUNK_SIZE)
                .then(move |results| {
                    future::ready(async move {
                        let ids: Vec<ChangesetId> = results
                            .into_iter()
                            .map(|r| r.map(|((id, _), _bounds)| id))
                            .collect::<Result<Vec<_>, Error>>()?;
                        let entries = self.changesets.get_many(ctx.clone(), ids.clone()).await?;
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
    ) -> impl Stream<Item = Result<((ChangesetId, u64), (u64, u64)), Error>> + 'a {
        let phases = self.phases.as_ref();
        let repo_bounds = if let Some(repo_bounds) = repo_bounds {
            future::ok(repo_bounds).left_future()
        } else {
            self.get_repo_bounds(ctx).right_future()
        };
        let step = self.step;
        let read_from_master = self.read_from_master;

        async move {
            let s = bounded_traversal_stream(
                1,
                Some(repo_bounds.await?),
                // Returns ids plus next bounds to query, if any
                move |(lower, upper): (u64, u64)| {
                    async move {
                        let next = {
                            let ctx = ctx.clone();
                            let changesets = self.changesets.clone();
                            async move {
                                let results: Vec<_> = changesets
                                    .list_enumeration_range(
                                        &ctx,
                                        lower,
                                        upper,
                                        Some((d.sort_order(), step)),
                                        read_from_master,
                                    )
                                    .try_collect()
                                    .await?;

                                let count = results.len() as u64;
                                let mut max_id = lower;
                                let mut min_id = upper - 1;
                                let cs_id_ids: Vec<(ChangesetId, u64)> = results
                                    .into_iter()
                                    .map(|(cs_id, id)| {
                                        max_id = max(max_id, id);
                                        min_id = min(min_id, id);
                                        (cs_id, id)
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

                                Ok::<_, Error>(((cs_id_ids, completed), new_bounds))
                            }
                        }
                        .boxed();
                        let handle = tokio::task::spawn(next);
                        handle.await?
                    }
                    .boxed()
                },
            )
            .and_then(move |(mut ids, completed_bounds)| async move {
                if !ids.is_empty() {
                    let cs_ids = ids.iter().map(|(cs_id, _)| *cs_id).collect();
                    let public = phases.get_cached_public(ctx, cs_ids).await?;
                    ids.retain(|(id, _)| public.contains(id));
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

    /// Get the repo bounds as max/min observed suitable for rust ranges
    pub async fn get_repo_bounds(&self, ctx: &CoreContext) -> Result<(u64, u64), Error> {
        self.get_repo_bounds_after_commits(ctx, vec![]).await
    }

    /// Get repo bounds for commits that arrived after the *newest* of the given
    /// commits. This is useful for getting a batch for a PrefetchedChangesetsFetcher
    /// used by a tailer.
    ///
    /// Note that this is permitted to not return all commits in that range
    pub async fn get_repo_bounds_after_commits(
        &self,
        ctx: &CoreContext,
        known_heads: Vec<ChangesetId>,
    ) -> Result<(u64, u64), Error> {
        let bounds = self
            .changesets
            .enumeration_bounds(ctx, self.read_from_master, known_heads)
            .await?;
        match bounds {
            // Add one to make the range half-open: [min, max).
            Some((min_id, max_id)) => Ok((min_id, max_id + 1)),
            None => Err(Error::msg("changesets table is empty")),
        }
    }
}

// Blobstore gets don't need batching as much as the SQL queries
const BLOBSTORE_CHUNK_SIZE: usize = 1000;

pub const MAX_FETCH_STEP: u64 = 65536;
pub const MIN_FETCH_STEP: u64 = 1;

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use blobrepo::BlobRepo;
    use bookmarks::BookmarkName;
    use changesets::ChangesetsArc;
    use fbinit::FacebookInit;
    use fixtures::BranchWide;
    use fixtures::TestRepoFixture;
    use mononoke_types::ChangesetId;
    use phases::PhasesArc;
    use phases::PhasesRef;

    use super::*;

    async fn get_test_repo(ctx: &CoreContext, fb: FacebookInit) -> Result<BlobRepo, Error> {
        let blobrepo = BranchWide::getrepo(fb).await;

        // our function avoids derivation so we need to explicitly do the derivation for
        // phases to have any data
        let master = BookmarkName::new("master")?;
        let master = blobrepo
            .bookmarks()
            .get(ctx.clone(), &master)
            .await?
            .unwrap();
        blobrepo
            .phases()
            .add_reachable_as_public(ctx, vec![master])
            .await?;

        Ok(blobrepo)
    }

    fn build_fetcher(
        step_size: u64,
        blobrepo: &BlobRepo,
    ) -> Result<PublicChangesetBulkFetch, Error> {
        PublicChangesetBulkFetch::new(blobrepo.changesets_arc(), blobrepo.phases_arc())
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
                    .map_ok(|((cs_id, _), (lower, upper))| {
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

            let repo_bounds: (u64, u64) = fetcher.get_repo_bounds(&ctx).await?;
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

    #[fbinit::test]
    async fn test_find_bounds_after_commits(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobrepo = get_test_repo(&ctx, fb).await?;

        let fetcher =
            PublicChangesetBulkFetch::new(blobrepo.changesets_arc(), blobrepo.phases_arc());
        // If we give empty known heads, we expect all IDs in the repo
        assert_eq!(
            (1, 8),
            fetcher.get_repo_bounds_after_commits(&ctx, vec![]).await?
        );

        // If I give it changeset 1 as known, I get 2 to 8
        assert_eq!(
            (2, 8),
            fetcher
                .get_repo_bounds_after_commits(
                    &ctx,
                    vec![ChangesetId::from_str(
                        "56c0203d7a9a83f14a47a17d3a10e55b1d08feb106fd72f28275e603c6e59625"
                    )?]
                )
                .await?
        );

        // If I give it changeset 3 as known, I get 4 to 8
        assert_eq!(
            (4, 8),
            fetcher
                .get_repo_bounds_after_commits(
                    &ctx,
                    vec![ChangesetId::from_str(
                        "624aba5e7f94c9319d949bce9f0dc87f25067f01f2ca1e41b620aff0625439c8"
                    )?]
                )
                .await?
        );

        // If I give it changesets 1 and 3 as known, I get 4 to 8
        // This misses out changeset 2, which is deliberate because changeset ID 3 is later.
        assert_eq!(
            (4, 8),
            fetcher
                .get_repo_bounds_after_commits(
                    &ctx,
                    vec![
                        ChangesetId::from_str(
                            "56c0203d7a9a83f14a47a17d3a10e55b1d08feb106fd72f28275e603c6e59625"
                        )?,
                        ChangesetId::from_str(
                            "624aba5e7f94c9319d949bce9f0dc87f25067f01f2ca1e41b620aff0625439c8"
                        )?
                    ]
                )
                .await?
        );

        Ok(())
    }
}
