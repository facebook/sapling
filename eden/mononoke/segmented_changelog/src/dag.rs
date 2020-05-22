/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, sync::Arc};

use anyhow::{format_err, Result};

use futures::stream::TryStreamExt;
use maplit::{hashmap, hashset};

use dag::{self, Id as Vertex, InProcessIdDag};

use bulkops::fetch_all_public_changesets;
use changesets::{ChangesetEntry, SqlChangesets};
use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};
use phases::SqlPhases;

use crate::idmap::IdMap;

// Note. The equivalent graph in the scm/lib/dag crate is `NameDag`.
pub struct Dag {
    // core fields
    repo_id: RepositoryId,
    iddag: InProcessIdDag,
    idmap: Arc<IdMap>,
}

impl Dag {
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

    pub async fn build_all_graph(
        &mut self,
        ctx: &CoreContext,
        changesets: &SqlChangesets,
        phases: &SqlPhases,
        head: ChangesetId,
    ) -> Result<()> {
        let changeset_entries: Vec<ChangesetEntry> =
            fetch_all_public_changesets(ctx, self.repo_id, changesets, phases)
                .try_collect()
                .await?;
        let mut parents_map: HashMap<ChangesetId, Vec<ChangesetId>> = HashMap::new();
        for cs_entry in changeset_entries.into_iter() {
            parents_map.insert(cs_entry.cs_id, cs_entry.parents);
        }

        let get_parents = |cs_id: ChangesetId| {
            parents_map
                .get(&cs_id)
                .ok_or_else(|| format_err!("commit {} not found in parents map", cs_id))
                .map(|v| v.clone())
            // TODO(sfilip): use a dedicated parents structure which specializes the case where
            // we have 0, 1 and 2 parents, 3+ is a 4th variant backed by Vec.
        };

        let low_vertex = dag::Group::MASTER.min_id().0;
        let (vertex2cs, cs2vertex) = assign_ids(low_vertex, head, &get_parents).await?;

        let head_vertex = *cs2vertex
            .get(&head)
            .ok_or_else(|| format_err!("error building IdMap; failed to assign head {}", head))?;

        // TODO(sfilip): batch operations
        for (vertex, cs_id) in vertex2cs.iter() {
            self.idmap.insert(self.repo_id, *vertex, *cs_id).await?;
        }

        let high_vertex = low_vertex + vertex2cs.len() as u64;
        let get_parents_vertex = |vertex: Vertex| -> Result<Vec<Vertex>> {
            if low_vertex > vertex.0 || vertex.0 > high_vertex {
                return Err(format_err!(
                    "invalid Vertex requested by IdDag: {}; valid Vertex range: [{}, {}]",
                    vertex,
                    low_vertex,
                    high_vertex,
                ));
            }
            let cs_id = *vertex2cs.get(&vertex).ok_or_else(|| {
                format_err!("failed to find vertex {} mapping in vertex2cs", vertex)
            })?;
            let parents = get_parents(cs_id)?;
            let mut response = Vec::with_capacity(parents.len());
            for parent in parents {
                let vertex = *cs2vertex.get(&parent).ok_or_else(|| {
                    format_err!("failed to find changeset {} mapping in cs2vertex", parent)
                })?;
                response.push(vertex);
            }
            Ok(response)
        };

        self.iddag
            .build_segments_volatile(head_vertex, &get_parents_vertex)?;

        Ok(())
    }
}

async fn assign_ids<F>(
    low_vertex: u64,
    head: ChangesetId,
    get_parents: &F,
) -> Result<(HashMap<Vertex, ChangesetId>, HashMap<ChangesetId, Vertex>)>
where
    F: Fn(ChangesetId) -> Result<Vec<ChangesetId>>,
{
    enum Todo {
        Visit(ChangesetId),
        Assign(ChangesetId),
    }
    let mut todo_stack = vec![Todo::Visit(head)];
    let mut vertex2cs = hashmap![];
    let mut cs2vertex = hashmap![];
    let mut seen = hashset![head];

    while let Some(todo) = todo_stack.pop() {
        match todo {
            Todo::Visit(cs_id) => {
                todo_stack.push(Todo::Assign(cs_id));
                let parents = get_parents(cs_id)?;
                for parent in parents.iter().rev() {
                    // Note: iterating parents in reverse is a small optimization because
                    // in our setup p1 is master.
                    if !seen.contains(parent) {
                        seen.insert(*parent);
                        todo_stack.push(Todo::Visit(*parent));
                    }
                }
            }
            Todo::Assign(cs_id) => {
                let vertex = Vertex(low_vertex + vertex2cs.len() as u64);
                vertex2cs.insert(vertex, cs_id);
                cs2vertex.insert(cs_id, vertex);
            }
        }
    }

    Ok((vertex2cs, cs2vertex))
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;

    use blobrepo::BlobRepo;
    use fixtures::{linear, merge_even, merge_uneven};
    use futures::compat::{Future01CompatExt, Stream01CompatExt};
    use futures::StreamExt;
    use phases::mark_reachable_as_public;
    use revset::AncestorsNodeStream;
    use sql_construct::SqlConstruct;
    use tests_utils::resolve_cs_id;

    impl Dag {
        fn new_in_process(repo_id: RepositoryId) -> Result<Dag> {
            Ok(Dag {
                repo_id,
                iddag: InProcessIdDag::new_in_process(),
                idmap: Arc::new(IdMap::with_sqlite_in_memory()?),
            })
        }

        async fn build_from_blobrepo(
            &mut self,
            ctx: &CoreContext,
            blobrepo: &BlobRepo,
            head: ChangesetId,
        ) -> Result<()> {
            let changesets = blobrepo.get_changesets_object();
            let sql_changesets = changesets.get_sql_changesets();
            let phases = blobrepo.get_phases();
            let sql_phases = phases.get_sql_phases();
            self.build_all_graph(ctx, sql_changesets, sql_phases, head)
                .await?;
            Ok(())
        }

        async fn new_build_from_blobrepo(
            ctx: &CoreContext,
            blobrepo: &BlobRepo,
            head: ChangesetId,
        ) -> Result<Dag> {
            let mut dag = Self::new_in_process(blobrepo.get_repoid())?;
            dag.build_from_blobrepo(ctx, blobrepo, head).await?;
            Ok(dag)
        }
    }

    async fn setup_phases(ctx: &CoreContext, blobrepo: &BlobRepo, head: ChangesetId) -> Result<()> {
        let phases = blobrepo.get_phases();
        let sql_phases = phases.get_sql_phases();
        mark_reachable_as_public(&ctx, sql_phases, &[head], false).await?;
        Ok(())
    }

    async fn validate_build_up_idmap(
        ctx: CoreContext,
        blobrepo: BlobRepo,
        head: &'static str,
    ) -> Result<()> {
        let head = resolve_cs_id(&ctx, &blobrepo, head).await?;
        setup_phases(&ctx, &blobrepo, head).await?;
        let dag = Dag::new_build_from_blobrepo(&ctx, &blobrepo, head).await?;

        let mut ancestors =
            AncestorsNodeStream::new(ctx.clone(), &blobrepo.get_changeset_fetcher(), head).compat();
        while let Some(cs_id) = ancestors.next().await {
            let cs_id = cs_id?;
            let parents = blobrepo
                .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
                .compat()
                .await?;
            for parent in parents {
                let parent_vertex = dag.idmap.get_vertex(blobrepo.get_repoid(), parent).await?;
                let vertex = dag.idmap.get_vertex(blobrepo.get_repoid(), cs_id).await?;
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
        blobrepo: BlobRepo,
        known: &'static str,
        distance: u64,
        expected: &'static str,
    ) -> Result<()> {
        let known_cs_id = resolve_cs_id(&ctx, &blobrepo, known).await?;
        setup_phases(&ctx, &blobrepo, known_cs_id).await?;
        let dag = Dag::new_build_from_blobrepo(&ctx, &blobrepo, known_cs_id).await?;

        let answer = dag.location_to_changeset_id(known_cs_id, distance).await?;
        let expected_cs_id = resolve_cs_id(&ctx, &blobrepo, expected).await?;
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
        let blobrepo = linear::getrepo(fb).await;
        let mut dag = Dag::new_in_process(blobrepo.get_repoid())?;

        let known_cs =
            resolve_cs_id(&ctx, &blobrepo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
        setup_phases(&ctx, &blobrepo, known_cs).await?;
        dag.build_from_blobrepo(&ctx, &blobrepo, known_cs).await?;
        let distance: u64 = 2;
        let answer = dag.location_to_changeset_id(known_cs, distance).await?;
        let expected_cs =
            resolve_cs_id(&ctx, &blobrepo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;
        assert_eq!(answer, expected_cs);

        let known_cs =
            resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        setup_phases(&ctx, &blobrepo, known_cs).await?;
        dag.build_from_blobrepo(&ctx, &blobrepo, known_cs).await?;
        let distance: u64 = 3;
        let answer = dag.location_to_changeset_id(known_cs, distance).await?;
        let expected_cs =
            resolve_cs_id(&ctx, &blobrepo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
        assert_eq!(answer, expected_cs);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_two_repo_dags(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let blobrepo1 = linear::getrepo(fb).await;
        let known_cs1 =
            resolve_cs_id(&ctx, &blobrepo1, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
        setup_phases(&ctx, &blobrepo1, known_cs1).await?;
        let dag1 = Dag::new_build_from_blobrepo(&ctx, &blobrepo1, known_cs1).await?;

        let blobrepo2 = merge_even::getrepo(fb).await;
        let known_cs2 =
            resolve_cs_id(&ctx, &blobrepo2, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await?;
        setup_phases(&ctx, &blobrepo2, known_cs2).await?;
        let dag2 = Dag::new_build_from_blobrepo(&ctx, &blobrepo2, known_cs2).await?;

        let distance: u64 = 4;
        let answer = dag1.location_to_changeset_id(known_cs1, distance).await?;
        let expected_cs_id =
            resolve_cs_id(&ctx, &blobrepo1, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        assert_eq!(answer, expected_cs_id);

        let distance: u64 = 2;
        let answer = dag2.location_to_changeset_id(known_cs2, distance).await?;
        let expected_cs_id =
            resolve_cs_id(&ctx, &blobrepo2, "d7542c9db7f4c77dab4b315edd328edf1514952f").await?;
        assert_eq!(answer, expected_cs_id);

        Ok(())
    }
}
