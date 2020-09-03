/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{format_err, Result};
use async_trait::async_trait;
use futures::{
    compat::Future01CompatExt,
    stream::{self, FuturesOrdered, StreamExt, TryStreamExt},
    try_join,
};
use maplit::hashset;
use tokio::sync::RwLock;

use dag::{self, Id as Vertex, InProcessIdDag};
use stats::prelude::*;

use bulkops::fetch_all_public_changesets;
use changeset_fetcher::ChangesetFetcher;
use changesets::{ChangesetEntry, SqlChangesets};
use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};
use phases::SqlPhases;

use crate::idmap::{IdMap, MemIdMap};
use crate::SegmentedChangelog;

const IDMAP_CHANGESET_FETCH_BATCH: usize = 500;

define_stats! {
    prefix = "mononoke.segmented_changelog";
    build_all_graph: timeseries(Sum),
    build_incremental: timeseries(Sum),
    location_to_changeset_id: timeseries(Sum),
}

pub struct OnDemandUpdateDag {
    dag: RwLock<Dag>,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
}

impl OnDemandUpdateDag {
    pub fn new(dag: Dag, changeset_fetcher: Arc<dyn ChangesetFetcher>) -> Self {
        Self {
            dag: RwLock::new(dag),
            changeset_fetcher,
        }
    }
}

#[async_trait]
impl SegmentedChangelog for OnDemandUpdateDag {
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        known: ChangesetId,
        distance: u64,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        {
            let dag = self.dag.read().await;
            if let Some(known_vertex) = dag.idmap.find_vertex(ctx, known).await? {
                return dag
                    .known_location_to_many_changeset_ids(ctx, known_vertex, distance, count)
                    .await;
            }
        }
        let known_vertex = {
            let mut dag = self.dag.write().await;
            dag.build_incremental(ctx, &self.changeset_fetcher, known)
                .await?
        };
        let dag = self.dag.read().await;
        dag.known_location_to_many_changeset_ids(ctx, known_vertex, distance, count)
            .await
    }
}

// Note. The equivalent graph in the scm/lib/dag crate is `NameDag`.
pub struct Dag {
    // core fields
    pub(crate) repo_id: RepositoryId,
    pub(crate) iddag: InProcessIdDag,
    pub(crate) idmap: Arc<dyn IdMap>,
}

#[async_trait]
impl SegmentedChangelog for Dag {
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        known: ChangesetId,
        distance: u64,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        let known_vertex = self.idmap.get_vertex(ctx, known).await?;
        self.known_location_to_many_changeset_ids(ctx, known_vertex, distance, count)
            .await
    }
}

impl Dag {
    pub fn new(repo_id: RepositoryId, iddag: InProcessIdDag, idmap: Arc<dyn IdMap>) -> Self {
        Self {
            repo_id,
            iddag,
            idmap,
        }
    }

    async fn known_location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        known_vertex: Vertex,
        distance: u64,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        let mut dist_ancestor_vertex = self.iddag.first_ancestor_nth(known_vertex, distance)?;
        let mut vertexes = vec![dist_ancestor_vertex];
        for _ in 1..count {
            let parents = self.iddag.parent_ids(dist_ancestor_vertex)?;
            if parents.len() != 1 {
                return Err(format_err!(
                    "invalid request: changeset with vertex {} does not have {} single parent ancestors",
                    known_vertex,
                    distance + count - 1
                ));
            }
            dist_ancestor_vertex = parents[0];
            vertexes.push(dist_ancestor_vertex);
        }
        let changeset_futures = vertexes
            .into_iter()
            .map(|vertex| self.idmap.get_changeset_id(ctx, vertex));
        stream::iter(changeset_futures)
            .buffered(IDMAP_CHANGESET_FETCH_BATCH)
            .try_collect()
            .await
    }

    pub async fn build_all_graph(
        &mut self,
        ctx: &CoreContext,
        changesets: &SqlChangesets,
        phases: &SqlPhases,
        head: ChangesetId,
    ) -> Result<Vertex> {
        STATS::build_all_graph.add_value(1);
        let changeset_entries: Vec<ChangesetEntry> =
            fetch_all_public_changesets(ctx, self.repo_id, changesets, phases)
                .try_collect()
                .await?;
        let mut start_state = StartState::new();
        for cs_entry in changeset_entries.into_iter() {
            start_state.insert_parents(cs_entry.cs_id, cs_entry.parents);
        }

        let low_vertex = dag::Group::MASTER.min_id();
        // TODO(sfilip, T67734329): monitor MySql lag replication
        self.build(ctx, low_vertex, head, start_state).await
    }

    pub async fn build_incremental(
        &mut self,
        ctx: &CoreContext,
        changeset_fetcher: &dyn ChangesetFetcher,
        head: ChangesetId,
    ) -> Result<Vertex> {
        STATS::build_incremental.add_value(1);
        let mut visited = HashSet::new();
        let mut start_state = StartState::new();
        {
            let mut queue = FuturesOrdered::new();
            queue.push(self.get_parents_and_vertex(ctx, changeset_fetcher, head));

            while let Some(entry) = queue.next().await {
                let (cs_id, parents, vertex) = entry?;
                start_state.insert_parents(cs_id, parents.clone());
                match vertex {
                    Some(v) => {
                        if cs_id == head {
                            return Ok(v);
                        }
                        start_state.insert_vertex_assignment(cs_id, v);
                    }
                    None => {
                        for parent in parents {
                            if visited.insert(parent) {
                                queue.push(self.get_parents_and_vertex(
                                    ctx,
                                    changeset_fetcher,
                                    parent,
                                ));
                            }
                        }
                    }
                }
            }
        }

        let low_vertex = self
            .idmap
            .get_last_entry(ctx)
            .await?
            .map_or_else(|| dag::Group::MASTER.min_id(), |(vertex, _)| vertex + 1);

        self.build(ctx, low_vertex, head, start_state).await
    }

    async fn get_parents_and_vertex(
        &self,
        ctx: &CoreContext,
        changeset_fetcher: &dyn ChangesetFetcher,
        cs_id: ChangesetId,
    ) -> Result<(ChangesetId, Vec<ChangesetId>, Option<Vertex>)> {
        let (parents, vertex) = try_join!(
            changeset_fetcher.get_parents(ctx.clone(), cs_id).compat(),
            self.idmap.find_vertex(ctx, cs_id)
        )?;
        Ok((cs_id, parents, vertex))
    }

    async fn build(
        &mut self,
        ctx: &CoreContext,
        low_vertex: Vertex,
        head: ChangesetId,
        start_state: StartState,
    ) -> Result<Vertex> {
        enum Todo {
            Visit(ChangesetId),
            Assign(ChangesetId),
        }
        let mut todo_stack = vec![Todo::Visit(head)];
        let mut mem_idmap = MemIdMap::new();
        let mut seen = hashset![head];

        while let Some(todo) = todo_stack.pop() {
            match todo {
                Todo::Visit(cs_id) => {
                    let parents = match start_state.get_parents_if_not_assigned(cs_id) {
                        None => continue,
                        Some(v) => v,
                    };
                    todo_stack.push(Todo::Assign(cs_id));
                    for parent in parents.iter().rev() {
                        // Note: iterating parents in reverse is a small optimization because
                        // in our setup p1 is master.
                        if seen.insert(*parent) {
                            todo_stack.push(Todo::Visit(*parent));
                        }
                    }
                }
                Todo::Assign(cs_id) => {
                    let vertex = low_vertex + mem_idmap.len() as u64;
                    mem_idmap.insert(vertex, cs_id);
                }
            }
        }

        let head_vertex = mem_idmap
            .find_vertex(head)
            .ok_or_else(|| format_err!("error building IdMap; failed to assign head {}", head))?;

        self.idmap
            .insert_many(ctx, mem_idmap.iter().collect::<Vec<_>>())
            .await?;

        let get_vertex_parents = |vertex: Vertex| -> dag::Result<Vec<Vertex>> {
            let cs_id = match mem_idmap.find_changeset_id(vertex) {
                None => start_state
                    .assignments
                    .get_changeset_id(vertex)
                    .map_err(|e| dag::errors::BackendError::Other(e))?,
                Some(v) => v,
            };
            let parents = start_state.parents.get(&cs_id).ok_or_else(|| {
                let err = format_err!(
                    "error building IdMap; unexpected request for parents for {}",
                    cs_id
                );
                dag::errors::BackendError::Other(err)
            })?;
            let mut response = Vec::with_capacity(parents.len());
            for parent in parents {
                let vertex = match mem_idmap.find_vertex(*parent) {
                    None => start_state
                        .assignments
                        .get_vertex(*parent)
                        .map_err(|e| dag::errors::BackendError::Other(e))?,
                    Some(v) => v,
                };
                response.push(vertex);
            }
            Ok(response)
        };

        // TODO(sfilip, T67731559): Prefetch parents for IdDag from last processed Vertex
        self.iddag
            .build_segments_volatile(head_vertex, &get_vertex_parents)?;

        Ok(head_vertex)
    }
}

// TODO(sfilip): use a dedicated parents structure which specializes the case where
// we have 0, 1 and 2 parents, 3+ is a 4th variant backed by Vec.
// Note: the segment construction algorithm will want to query the vertexes of the parents
// that were already assigned.
struct StartState {
    parents: HashMap<ChangesetId, Vec<ChangesetId>>,
    assignments: MemIdMap,
}

impl StartState {
    pub fn new() -> Self {
        Self {
            parents: HashMap::new(),
            assignments: MemIdMap::new(),
        }
    }

    pub fn insert_parents(
        &mut self,
        cs_id: ChangesetId,
        parents: Vec<ChangesetId>,
    ) -> Option<Vec<ChangesetId>> {
        self.parents.insert(cs_id, parents)
    }

    pub fn insert_vertex_assignment(&mut self, cs_id: ChangesetId, vertex: Vertex) {
        self.assignments.insert(vertex, cs_id)
    }

    // The purpose of the None return value is to signal that the changeset has already been assigned
    // This is useful in the incremental build step when we traverse back through parents. Normally
    // we would check the idmap at each iteration step but we have the information prefetched when
    // getting parents data.
    pub fn get_parents_if_not_assigned(&self, cs_id: ChangesetId) -> Option<Vec<ChangesetId>> {
        if self.assignments.find_vertex(cs_id).is_some() {
            return None;
        }
        self.parents.get(&cs_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;

    use blobrepo::BlobRepo;
    use fixtures::{linear, merge_even, merge_uneven, unshared_merge_even};
    use futures::compat::{Future01CompatExt, Stream01CompatExt};
    use futures::future::try_join_all;
    use futures::StreamExt;
    use phases::mark_reachable_as_public;
    use revset::AncestorsNodeStream;
    use sql_construct::SqlConstruct;
    use tests_utils::resolve_cs_id;

    use crate::builder::SegmentedChangelogBuilder;

    impl Dag {
        async fn build_all_from_blobrepo(
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

        async fn build_incremental_from_blobrepo(
            &mut self,
            ctx: &CoreContext,
            blobrepo: &BlobRepo,
            head: ChangesetId,
        ) -> Result<()> {
            let changeset_fetcher = blobrepo.get_changeset_fetcher();
            self.build_incremental(ctx, &*changeset_fetcher, head)
                .await?;
            Ok(())
        }

        async fn new_build_all_from_blobrepo(
            ctx: &CoreContext,
            blobrepo: &BlobRepo,
            head: ChangesetId,
        ) -> Result<Dag> {
            let mut dag = Self::new_in_process(blobrepo.get_repoid())?;
            dag.build_all_from_blobrepo(ctx, blobrepo, head).await?;
            Ok(dag)
        }

        pub fn new_in_process(repo_id: RepositoryId) -> Result<Dag> {
            SegmentedChangelogBuilder::with_sqlite_in_memory()?
                .with_repo_id(repo_id)
                .build_read_only()
        }
    }

    async fn setup_phases(ctx: &CoreContext, blobrepo: &BlobRepo, head: ChangesetId) -> Result<()> {
        let phases = blobrepo.get_phases();
        let sql_phases = phases.get_sql_phases();
        mark_reachable_as_public(&ctx, sql_phases, &[head], false).await?;
        Ok(())
    }

    async fn validate_build_idmap(
        ctx: CoreContext,
        blobrepo: BlobRepo,
        head: &'static str,
    ) -> Result<()> {
        let head = resolve_cs_id(&ctx, &blobrepo, head).await?;
        setup_phases(&ctx, &blobrepo, head).await?;
        let dag = Dag::new_build_all_from_blobrepo(&ctx, &blobrepo, head).await?;

        let mut ancestors =
            AncestorsNodeStream::new(ctx.clone(), &blobrepo.get_changeset_fetcher(), head).compat();
        while let Some(cs_id) = ancestors.next().await {
            let cs_id = cs_id?;
            let parents = blobrepo
                .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
                .compat()
                .await?;
            for parent in parents {
                let parent_vertex = dag.idmap.get_vertex(&ctx, parent).await?;
                let vertex = dag.idmap.get_vertex(&ctx, cs_id).await?;
                assert!(parent_vertex < vertex);
            }
        }
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_build_idmap(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        validate_build_idmap(
            ctx.clone(),
            linear::getrepo(fb).await,
            "79a13814c5ce7330173ec04d279bf95ab3f652fb",
        )
        .await?;
        validate_build_idmap(
            ctx.clone(),
            merge_even::getrepo(fb).await,
            "4dcf230cd2f20577cb3e88ba52b73b376a2b3f69",
        )
        .await?;
        validate_build_idmap(
            ctx.clone(),
            merge_uneven::getrepo(fb).await,
            "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b",
        )
        .await?;
        Ok(())
    }

    async fn validate_location_to_changeset_ids(
        ctx: CoreContext,
        blobrepo: BlobRepo,
        known: &'static str,
        distance_count: (u64, u64),
        expected: Vec<&'static str>,
    ) -> Result<()> {
        let (distance, count) = distance_count;
        let known_cs_id = resolve_cs_id(&ctx, &blobrepo, known).await?;
        setup_phases(&ctx, &blobrepo, known_cs_id).await?;
        let dag = Dag::new_build_all_from_blobrepo(&ctx, &blobrepo, known_cs_id).await?;

        let answer = dag
            .location_to_many_changeset_ids(&ctx, known_cs_id, distance, count)
            .await?;
        let expected_cs_ids = try_join_all(
            expected
                .into_iter()
                .map(|id| resolve_cs_id(&ctx, &blobrepo, id)),
        )
        .await?;
        assert_eq!(answer, expected_cs_ids);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_location_to_changeset_ids(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        validate_location_to_changeset_ids(
            ctx.clone(),
            linear::getrepo(fb).await,
            "79a13814c5ce7330173ec04d279bf95ab3f652fb", // master commit, message: modified 10
            (4, 3),
            vec![
                "0ed509bf086fadcb8a8a5384dc3b550729b0fc17", // message: added 7
                "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b", // message: added 6
                "cb15ca4a43a59acff5388cea9648c162afde8372", // message: added 5
            ],
        )
        .await?;
        validate_location_to_changeset_ids(
            ctx.clone(),
            merge_uneven::getrepo(fb).await,
            "264f01429683b3dd8042cb3979e8bf37007118bc", // top of merged branch, message: Add 5
            (4, 2),
            vec![
                "795b8133cf375f6d68d27c6c23db24cd5d0cd00f", // message: Add 1
                "4f7f3fd428bec1a48f9314414b063c706d9c1aed", // bottom of branch
            ],
        )
        .await?;
        validate_location_to_changeset_ids(
            ctx.clone(),
            unshared_merge_even::getrepo(fb).await,
            "7fe9947f101acb4acf7d945e69f0d6ce76a81113", // master commit
            (1, 1),
            vec!["d592490c4386cdb3373dd93af04d563de199b2fb"], // parent of master, merge commit
        )
        .await?;
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_location_to_changeset_id_invalid_req(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobrepo = unshared_merge_even::getrepo(fb).await;
        let known_cs_id =
            resolve_cs_id(&ctx, &blobrepo, "7fe9947f101acb4acf7d945e69f0d6ce76a81113").await?;
        setup_phases(&ctx, &blobrepo, known_cs_id).await?;
        let dag = Dag::new_build_all_from_blobrepo(&ctx, &blobrepo, known_cs_id).await?;
        assert!(dag
            .location_to_many_changeset_ids(&ctx, known_cs_id, 1, 2)
            .await
            .is_err());
        // TODO(T74320664): Ideally LocationToHash should error when asked to go over merge commit.
        // The parents order is not well defined enough for this not to be ambiguous.
        assert!(dag
            .location_to_many_changeset_ids(&ctx, known_cs_id, 2, 1)
            .await
            .is_ok());
        let second_commit =
            resolve_cs_id(&ctx, &blobrepo, "1700524113b1a3b1806560341009684b4378660b").await?;
        assert!(dag
            .location_to_many_changeset_ids(&ctx, second_commit, 1, 2)
            .await
            .is_err());
        assert!(dag
            .location_to_many_changeset_ids(&ctx, second_commit, 2, 1)
            .await
            .is_err());
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_build_incremental_from_scratch(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        {
            // linear
            let blobrepo = linear::getrepo(fb).await;
            let mut dag = Dag::new_in_process(blobrepo.get_repoid())?;

            let known_cs =
                resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
            let vertex1 = dag
                .build_incremental_from_blobrepo(&ctx, &blobrepo, known_cs)
                .await?;
            let distance: u64 = 4;
            let answer = dag
                .location_to_changeset_id(&ctx, known_cs, distance)
                .await?;
            let expected_cs =
                resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
            assert_eq!(answer, expected_cs);
            let vertex2 = dag
                .build_incremental_from_blobrepo(&ctx, &blobrepo, known_cs)
                .await?;
            assert_eq!(vertex1, vertex2);
        }
        {
            // merge_uneven
            let blobrepo = merge_uneven::getrepo(fb).await;
            let mut dag = Dag::new_in_process(blobrepo.get_repoid())?;

            let known_cs =
                resolve_cs_id(&ctx, &blobrepo, "264f01429683b3dd8042cb3979e8bf37007118bc").await?;
            dag.build_incremental_from_blobrepo(&ctx, &blobrepo, known_cs)
                .await?;
            let distance: u64 = 5;
            let answer = dag
                .location_to_changeset_id(&ctx, known_cs, distance)
                .await?;
            let expected_cs =
                resolve_cs_id(&ctx, &blobrepo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await?;
            assert_eq!(answer, expected_cs);
        }

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_build_calls_together(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobrepo = linear::getrepo(fb).await;
        let mut dag = Dag::new_in_process(blobrepo.get_repoid())?;

        let known_cs =
            resolve_cs_id(&ctx, &blobrepo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
        setup_phases(&ctx, &blobrepo, known_cs).await?;
        dag.build_all_from_blobrepo(&ctx, &blobrepo, known_cs)
            .await?;
        let distance: u64 = 2;
        let answer = dag
            .location_to_changeset_id(&ctx, known_cs, distance)
            .await?;
        let expected_cs =
            resolve_cs_id(&ctx, &blobrepo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;
        assert_eq!(answer, expected_cs);

        let known_cs =
            resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        setup_phases(&ctx, &blobrepo, known_cs).await?;
        dag.build_incremental_from_blobrepo(&ctx, &blobrepo, known_cs)
            .await?;
        let distance: u64 = 3;
        let answer = dag
            .location_to_changeset_id(&ctx, known_cs, distance)
            .await?;
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
        let dag1 = Dag::new_build_all_from_blobrepo(&ctx, &blobrepo1, known_cs1).await?;

        let blobrepo2 = merge_even::getrepo(fb).await;
        let known_cs2 =
            resolve_cs_id(&ctx, &blobrepo2, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await?;
        setup_phases(&ctx, &blobrepo2, known_cs2).await?;
        let dag2 = Dag::new_build_all_from_blobrepo(&ctx, &blobrepo2, known_cs2).await?;

        let distance: u64 = 4;
        let answer = dag1
            .location_to_changeset_id(&ctx, known_cs1, distance)
            .await?;
        let expected_cs_id =
            resolve_cs_id(&ctx, &blobrepo1, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        assert_eq!(answer, expected_cs_id);

        let distance: u64 = 2;
        let answer = dag2
            .location_to_changeset_id(&ctx, known_cs2, distance)
            .await?;
        let expected_cs_id =
            resolve_cs_id(&ctx, &blobrepo2, "d7542c9db7f4c77dab4b315edd328edf1514952f").await?;
        assert_eq!(answer, expected_cs_id);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_on_demand_update_dag_location_to_changeset_ids(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobrepo = linear::getrepo(fb).await;

        let known_cs =
            resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;

        let dag = SegmentedChangelogBuilder::with_sqlite_in_memory()?
            .with_repo_id(blobrepo.get_repoid())
            .with_changeset_fetcher(blobrepo.get_changeset_fetcher())
            .build_on_demand_update()?;

        let distance: u64 = 4;
        let answer = dag
            .location_to_changeset_id(&ctx, known_cs, distance)
            .await?;
        let expected_cs =
            resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        assert_eq!(answer, expected_cs);

        Ok(())
    }
}
