/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use fbinit::FacebookInit;
use futures::compat::{Future01CompatExt, Stream01CompatExt};
use futures::future::try_join_all;
use futures::StreamExt;

use blobrepo::BlobRepo;
use context::CoreContext;
use dag::InProcessIdDag;
use fixtures::{linear, merge_even, merge_uneven, unshared_merge_even};
use mononoke_types::ChangesetId;
use phases::mark_reachable_as_public;
use revset::AncestorsNodeStream;
use sql_construct::SqlConstruct;
use tests_utils::resolve_cs_id;

use crate::builder::SegmentedChangelogBuilder;
use crate::dag::Dag;
use crate::iddag::IdDagSaveStore;
use crate::on_demand::OnDemandUpdateDag;
use crate::types::IdDagVersion;
use crate::SegmentedChangelog;

async fn validate_build_idmap(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    head: &'static str,
) -> Result<()> {
    let head = resolve_cs_id(&ctx, &blobrepo, head).await?;
    setup_phases(&ctx, &blobrepo, head).await?;
    let dag = new_build_all_from_blobrepo(&ctx, &blobrepo, head).await?;

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
pub async fn setup_phases(ctx: &CoreContext, blobrepo: &BlobRepo, head: ChangesetId) -> Result<()> {
    let phases = blobrepo.get_phases();
    let sql_phases = phases.get_sql_phases();
    mark_reachable_as_public(&ctx, sql_phases, &[head], false).await?;
    Ok(())
}

pub async fn new_build_all_from_blobrepo(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    head: ChangesetId,
) -> Result<Dag> {
    let seeder = SegmentedChangelogBuilder::with_sqlite_in_memory()?
        .with_blobrepo(blobrepo)
        .build_seeder(ctx)
        .await?;

    let (dag, _) = seeder.build_dag_from_scratch(ctx, head).await?;
    Ok(dag)
}

#[fbinit::compat_test]
async fn test_iddag_save_store(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = linear::getrepo(fb).await;
    let repo_id = blobrepo.get_repoid();

    let known_cs =
        resolve_cs_id(&ctx, &blobrepo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
    setup_phases(&ctx, &blobrepo, known_cs).await?;
    let dag = new_build_all_from_blobrepo(&ctx, &blobrepo, known_cs).await?;

    let distance: u64 = 2;
    let answer = dag
        .location_to_changeset_id(&ctx, known_cs, distance)
        .await?;
    let expected_cs =
        resolve_cs_id(&ctx, &blobrepo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;
    assert_eq!(answer, expected_cs);

    let iddag_save_store = IdDagSaveStore::new(repo_id, Arc::new(blobrepo.get_blobstore()));
    let iddag_version = iddag_save_store.save(&ctx, &dag.iddag).await?;

    assert!(
        iddag_save_store
            .find(&ctx, IdDagVersion::from_serialized_bytes(b"random"))
            .await?
            .is_none()
    );
    let loaded_id_dag = iddag_save_store.load(&ctx, iddag_version).await?;
    let from_save = Dag::new(loaded_id_dag, dag.idmap.clone());
    let answer = from_save
        .location_to_changeset_id(&ctx, known_cs, distance)
        .await?;
    assert_eq!(answer, expected_cs);

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
    let dag = new_build_all_from_blobrepo(&ctx, &blobrepo, known_cs_id).await?;

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
    let dag = new_build_all_from_blobrepo(&ctx, &blobrepo, known_cs_id).await?;
    assert!(
        dag.location_to_many_changeset_ids(&ctx, known_cs_id, 1u64, 2u64)
            .await
            .is_err()
    );
    // TODO(T74320664): Ideally LocationToHash should error when asked to go over merge commit.
    // The parents order is not well defined enough for this not to be ambiguous.
    assert!(
        dag.location_to_many_changeset_ids(&ctx, known_cs_id, 2u64, 1u64)
            .await
            .is_ok()
    );
    let second_commit =
        resolve_cs_id(&ctx, &blobrepo, "1700524113b1a3b1806560341009684b4378660b").await?;
    assert!(
        dag.location_to_many_changeset_ids(&ctx, second_commit, 1u64, 2u64)
            .await
            .is_err()
    );
    assert!(
        dag.location_to_many_changeset_ids(&ctx, second_commit, 2u64, 1u64)
            .await
            .is_err()
    );
    Ok(())
}

#[fbinit::compat_test]
async fn test_build_incremental_from_scratch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    {
        // linear
        let blobrepo = linear::getrepo(fb).await;
        let dag = SegmentedChangelogBuilder::with_sqlite_in_memory()?
            .with_blobrepo(&blobrepo)
            .build_on_demand_update()?;

        let known_cs =
            resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
        let distance: u64 = 4;
        let answer = dag
            .location_to_changeset_id(&ctx, known_cs, distance)
            .await?;
        let expected_cs =
            resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        assert_eq!(answer, expected_cs);
    }
    {
        // merge_uneven
        let blobrepo = merge_uneven::getrepo(fb).await;
        let dag = SegmentedChangelogBuilder::with_sqlite_in_memory()?
            .with_blobrepo(&blobrepo)
            .build_on_demand_update()?;

        let known_cs =
            resolve_cs_id(&ctx, &blobrepo, "264f01429683b3dd8042cb3979e8bf37007118bc").await?;
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

    let known_cs =
        resolve_cs_id(&ctx, &blobrepo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
    setup_phases(&ctx, &blobrepo, known_cs).await?;
    let dag = new_build_all_from_blobrepo(&ctx, &blobrepo, known_cs).await?;
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
    let on_demand_update_dag = OnDemandUpdateDag::new(dag, blobrepo.get_changeset_fetcher());
    let distance: u64 = 3;
    let answer = on_demand_update_dag
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
    let dag1 = new_build_all_from_blobrepo(&ctx, &blobrepo1, known_cs1).await?;

    let blobrepo2 = merge_even::getrepo(fb).await;
    let known_cs2 =
        resolve_cs_id(&ctx, &blobrepo2, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await?;
    setup_phases(&ctx, &blobrepo2, known_cs2).await?;
    let dag2 = new_build_all_from_blobrepo(&ctx, &blobrepo2, known_cs2).await?;

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
        .with_blobrepo(&blobrepo)
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

#[fbinit::compat_test]
async fn test_incremental_update_with_desync_iddag(fb: FacebookInit) -> Result<()> {
    // In this test we first build a dag from scratch and then we reuse the idmap in an ondemand
    // dag that starts off with an empty iddag.
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = linear::getrepo(fb).await;

    let master_cs =
        resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;

    setup_phases(&ctx, &blobrepo, master_cs).await?;

    let mut builder = SegmentedChangelogBuilder::with_sqlite_in_memory()?.with_blobrepo(&blobrepo);
    let seeder = builder.build_seeder(&ctx).await?;

    let (dag, _) = seeder.build_dag_from_scratch(&ctx, master_cs).await?;

    let cs7 = resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
    let distance: u64 = 4;
    let answer = dag
        .location_to_changeset_id(&ctx, master_cs, distance)
        .await?;
    assert_eq!(answer, cs7);

    let ondemand_dag = builder.build_on_demand_update()?;

    let cs3 = resolve_cs_id(&ctx, &blobrepo, "607314ef579bd2407752361ba1b0c1729d08b281").await?;
    let answer = ondemand_dag
        .location_to_changeset_id(&ctx, cs7, distance)
        .await?;
    assert_eq!(answer, cs3);

    let answer = ondemand_dag
        .location_to_changeset_id(&ctx, master_cs, distance)
        .await?;
    assert_eq!(answer, cs7);

    Ok(())
}

#[fbinit::compat_test]
async fn test_clone_data(fb: FacebookInit) -> Result<()> {
    // In this test we first build a dag from scratch and then we reuse the idmap in an ondemand
    // dag that starts off with an empty iddag.
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = linear::getrepo(fb).await;

    let head = resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
    setup_phases(&ctx, &blobrepo, head).await?;
    let dag = new_build_all_from_blobrepo(&ctx, &blobrepo, head).await?;

    let clone_data = dag.clone_data(&ctx).await?;

    let mut new_iddag = InProcessIdDag::new_in_process();
    new_iddag.build_segments_volatile_from_prepared_flat_segments(&clone_data.flat_segments)?;
    let new_dag = Dag::new(new_iddag, dag.idmap.clone());

    let distance: u64 = 4;
    let answer = new_dag
        .location_to_changeset_id(&ctx, head, distance)
        .await?;
    let expected_cs =
        resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
    assert_eq!(answer, expected_cs);

    Ok(())
}
