/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::{format_err, Result};
use futures::{
    compat::Future01CompatExt,
    future,
    stream::{self, StreamExt, TryStreamExt},
};
use maplit::hashset;

use dag::{self, Id as Vertex, InProcessIdDag, Level};

use blobrepo::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};
use sql_construct::SqlConstruct;

#[cfg(test)]
use blobrepo::BlobRepo;

use crate::idmap::IdMap;

// Note. The equivalent graph in the scm/lib/dag crate is `NameDag`.
pub struct Dag {
    // core fields
    repo_id: RepositoryId,
    iddag: InProcessIdDag,
    // dependencies
    idmap: Arc<IdMap>,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
}

impl Dag {
    // Dummy method. A production setup would have the changeset built by a separate job.
    pub async fn build_up(&mut self, ctx: &CoreContext, head: ChangesetId) -> Result<()> {
        let high_vertex = self.build_up_idmap(ctx, head).await?;
        let low_vertex = self.iddag.next_free_id(0 as Level, high_vertex.group())?;
        if low_vertex >= high_vertex {
            return Ok(());
        }
        let idmap = &self.idmap;

        // TODO(sfilip): buffering
        let parents: Vec<Vec<Vertex>> = stream::iter(low_vertex.to(high_vertex))
            .map(Ok)
            .and_then(|vertex| idmap.get_changeset_id(self.repo_id, vertex))
            .and_then(|cs_id: ChangesetId| {
                self.changeset_fetcher
                    .get_parents(ctx.clone(), cs_id)
                    .compat()
            })
            .and_then(|cs_ids| {
                future::try_join_all(
                    cs_ids
                        .iter()
                        .map(|cs_id| idmap.get_vertex(self.repo_id, *cs_id)),
                )
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
        let known_vertex = self.idmap.get_vertex(self.repo_id, known).await?;
        let dist_ancestor_vertex = self.iddag.first_ancestor_nth(known_vertex, distance)?;
        let dist_ancestor = self
            .idmap
            .get_changeset_id(self.repo_id, dist_ancestor_vertex)
            .await?;
        Ok(dist_ancestor)
    }

    pub(crate) async fn build_up_idmap(
        &self,
        ctx: &CoreContext,
        head: ChangesetId,
    ) -> Result<Vertex> {
        enum Todo {
            Visit(ChangesetId),
            Assign(ChangesetId),
        }
        let mut next_vertex = dag::Group::MASTER.min_id().0;
        let mut todo_stack = vec![Todo::Visit(head)];
        let mut seen = hashset![head];
        while let Some(todo) = todo_stack.pop() {
            match todo {
                Todo::Visit(cs_id) => {
                    todo_stack.push(Todo::Assign(cs_id));
                    let parents = self
                        .changeset_fetcher
                        .get_parents(ctx.clone(), cs_id)
                        .compat()
                        .await?;
                    for parent in parents.into_iter().rev() {
                        // Note: iterating parents in reverse is a small optimization because
                        // in our setup p1 is master.
                        if !seen.contains(&parent) {
                            seen.insert(parent);
                            todo_stack.push(Todo::Visit(parent));
                        }
                    }
                }
                Todo::Assign(cs_id) => {
                    self.idmap
                        .insert(self.repo_id, Vertex(next_vertex), cs_id)
                        .await?;
                    next_vertex += 1;
                }
            }
        }
        match self.idmap.find_vertex(self.repo_id, head).await? {
            None => Err(format_err!(
                "Error building IdMap. Failed to assign head {}",
                head
            )),
            Some(vertex) => Ok(vertex),
        }
    }

    #[cfg(test)]
    pub fn new_in_process(blobrepo: &BlobRepo) -> Result<Self> {
        Ok(Dag {
            repo_id: blobrepo.get_repoid(),
            iddag: InProcessIdDag::new_in_process(),
            idmap: Arc::new(IdMap::with_sqlite_in_memory()?),
            changeset_fetcher: blobrepo.get_changeset_fetcher(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;

    use fixtures::{linear, merge_even, merge_uneven};
    use futures::compat::{Future01CompatExt, Stream01CompatExt};
    use futures::StreamExt;
    use revset::AncestorsNodeStream;
    use tests_utils::resolve_cs_id;

    async fn validate_build_up_idmap(
        ctx: CoreContext,
        repo: BlobRepo,
        head: &'static str,
    ) -> Result<()> {
        let dag = Dag::new_in_process(&repo)?;
        let head = resolve_cs_id(&ctx, &repo, head).await?;
        dag.build_up_idmap(&ctx, head).await?;

        let mut ancestors =
            AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), head).compat();
        while let Some(cs_id) = ancestors.next().await {
            let cs_id = cs_id?;
            let parents = repo
                .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
                .compat()
                .await?;
            for parent in parents {
                let parent_vertex = dag.idmap.get_vertex(repo.get_repoid(), parent).await?;
                let vertex = dag.idmap.get_vertex(repo.get_repoid(), cs_id).await?;
                assert!(parent_vertex < vertex);
            }
        }
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_build_up_idmap(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        validate_build_up_idmap(
            ctx.clone(),
            linear::getrepo(fb).await,
            "79a13814c5ce7330173ec04d279bf95ab3f652fb",
        )
        .await?;
        validate_build_up_idmap(
            ctx.clone(),
            merge_even::getrepo(fb).await,
            "4dcf230cd2f20577cb3e88ba52b73b376a2b3f69",
        )
        .await?;
        validate_build_up_idmap(
            ctx.clone(),
            merge_uneven::getrepo(fb).await,
            "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b",
        )
        .await?;
        Ok(())
    }

    async fn validate_location_to_changeset_id(
        ctx: CoreContext,
        repo: BlobRepo,
        known: &'static str,
        distance: u64,
        expected: &'static str,
    ) -> Result<()> {
        let known_cs_id = resolve_cs_id(&ctx, &repo, known).await?;

        let mut dag = Dag::new_in_process(&repo)?;
        dag.build_up(&ctx, known_cs_id).await?;

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
        let mut dag = Dag::new_in_process(&repo)?;

        let known_cs =
            resolve_cs_id(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
        dag.build_up(&ctx, known_cs).await?;
        let distance = 2;
        let answer = dag.location_to_changeset_id(known_cs, distance).await?;
        let expected_cs =
            resolve_cs_id(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;
        assert_eq!(answer, expected_cs);

        let known_cs =
            resolve_cs_id(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        dag.build_up(&ctx, known_cs).await?;
        let distance = 3;
        let answer = dag.location_to_changeset_id(known_cs, distance).await?;
        let expected_cs =
            resolve_cs_id(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
        assert_eq!(answer, expected_cs);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_two_repo_dags(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let repo1 = linear::getrepo(fb).await;
        let mut dag1 = Dag::new_in_process(&repo1)?;

        let repo2 = merge_even::getrepo(fb).await;
        let mut dag2 = Dag::new_in_process(&repo2)?;

        let known_cs1 =
            resolve_cs_id(&ctx, &repo1, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
        dag1.build_up(&ctx, known_cs1).await?;

        let known_cs2 =
            resolve_cs_id(&ctx, &repo2, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await?;
        dag2.build_up(&ctx, known_cs2).await?;

        let answer = dag1.location_to_changeset_id(known_cs1, 4).await?;
        let expected_cs_id =
            resolve_cs_id(&ctx, &repo1, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        assert_eq!(answer, expected_cs_id);

        let answer = dag2.location_to_changeset_id(known_cs2, 2).await?;
        let expected_cs_id =
            resolve_cs_id(&ctx, &repo2, "d7542c9db7f4c77dab4b315edd328edf1514952f").await?;
        assert_eq!(answer, expected_cs_id);

        Ok(())
    }
}
