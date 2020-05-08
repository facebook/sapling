/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Result};
use futures::{
    future,
    stream::{self, StreamExt, TryStreamExt},
};

use dag::{self, Id as Vertex, InProcessIdDag, Level};

use blobrepo::BlobRepo;
use context::CoreContext;
use mononoke_types::ChangesetId;
use sql_construct::SqlConstruct;

use crate::{idmap::IdMap, parents::Parents};

// Note. The equivalent graph in the scm/lib/dag crate is `NameDag`.
pub struct Dag {
    idmap: IdMap,
    iddag: InProcessIdDag,
}

impl Dag {
    pub fn new_in_process() -> Result<Self> {
        let idmap = IdMap::with_sqlite_in_memory()?;
        let iddag = InProcessIdDag::new_in_process();
        Ok(Dag { idmap, iddag })
    }

    // Dummy method. A production setup would have the changeset built by a separate job.
    pub async fn build_up(
        &mut self,
        ctx: &CoreContext,
        blob_repo: &BlobRepo,
        head: ChangesetId,
    ) -> Result<()> {
        let high_vertex = self.idmap.build_up(ctx, blob_repo, head).await?;
        let low_vertex = self.iddag.next_free_id(0 as Level, high_vertex.group())?;
        if low_vertex >= high_vertex {
            return Ok(());
        }
        let idmap = &self.idmap;

        let parents_fetcher = Parents::new(ctx, blob_repo);

        // TODO(sfilip): buffering
        let parents: Vec<Vec<Vertex>> = stream::iter(low_vertex.to(high_vertex))
            .map(Ok)
            .and_then(|vertex| idmap.get_changeset_id(vertex))
            .and_then(|cs_id: ChangesetId| parents_fetcher.get(cs_id))
            .and_then(|cs_ids| {
                future::try_join_all(cs_ids.iter().map(|cs_id| idmap.get_vertex(*cs_id)))
            })
            .try_collect()
            .await?;
        let get_parents = |vertex: Vertex| {
            parents
                .get((vertex.0 - low_vertex.0) as usize)
                .cloned()
                .ok_or_else(|| {
                    format_err!(
                        "invalid Id requested by IdDag: {}; present Id range: [{}, {}]",
                        vertex,
                        low_vertex,
                        high_vertex
                    )
                })
        };
        // TODO(sfilip): check return value from build_segments_volatile
        self.iddag
            .build_segments_volatile(high_vertex, &get_parents)?;
        Ok(())
    }

    // TODO(sfilip): error scenarios
    pub async fn location_to_changeset_id(
        &self,
        known: ChangesetId,
        distance: u64,
    ) -> Result<ChangesetId> {
        let known_vertex = self.idmap.get_vertex(known).await?;
        let dist_ancestor_vertex = self.iddag.first_ancestor_nth(known_vertex, distance)?;
        let dist_ancestor = self.idmap.get_changeset_id(dist_ancestor_vertex).await?;
        Ok(dist_ancestor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;

    use fixtures::{linear, merge_even, merge_uneven};
    use tests_utils::resolve_cs_id;

    async fn validate_location_to_changeset_id(
        ctx: CoreContext,
        repo: BlobRepo,
        known: &'static str,
        distance: u64,
        expected: &'static str,
    ) -> Result<()> {
        let known_cs_id = resolve_cs_id(&ctx, &repo, known).await?;
        let mut dag = Dag::new_in_process()?;
        dag.build_up(&ctx, &repo, known_cs_id).await?;

        let answer = dag.location_to_changeset_id(known_cs_id, distance).await?;
        let expected_cs_id = resolve_cs_id(&ctx, &repo, expected).await?;
        assert_eq!(answer, expected_cs_id);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_location_to_changeset_id(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        validate_location_to_changeset_id(
            ctx.clone(),
            linear::getrepo(fb).await,
            "79a13814c5ce7330173ec04d279bf95ab3f652fb",
            4,
            "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
        )
        .await?;
        validate_location_to_changeset_id(
            ctx.clone(),
            merge_even::getrepo(fb).await,
            "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
            2,
            "d7542c9db7f4c77dab4b315edd328edf1514952f",
        )
        .await?;
        validate_location_to_changeset_id(
            ctx.clone(),
            merge_uneven::getrepo(fb).await,
            "264f01429683b3dd8042cb3979e8bf37007118bc",
            5,
            "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
        )
        .await?;
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_two_build_up_calls(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;
        let mut dag = Dag::new_in_process()?;

        let known_cs =
            resolve_cs_id(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
        dag.build_up(&ctx, &repo, known_cs).await?;
        let distance = 2;
        let answer = dag.location_to_changeset_id(known_cs, distance).await?;
        let expected_cs =
            resolve_cs_id(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;
        assert_eq!(answer, expected_cs);

        let known_cs =
            resolve_cs_id(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        dag.build_up(&ctx, &repo, known_cs).await?;
        let distance = 3;
        let answer = dag.location_to_changeset_id(known_cs, distance).await?;
        let expected_cs =
            resolve_cs_id(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
        assert_eq!(answer, expected_cs);

        Ok(())
    }
}
