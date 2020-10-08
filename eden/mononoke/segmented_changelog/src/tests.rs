/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use fbinit::FacebookInit;

use blobrepo::BlobRepo;
use context::CoreContext;
use fixtures::linear;
use mononoke_types::ChangesetId;
use phases::mark_reachable_as_public;
use tests_utils::resolve_cs_id;

use crate::dag::Dag;
use crate::iddag::IdDagSaveStore;
use crate::types::IdDagVersion;
use crate::SegmentedChangelog;

pub async fn setup_phases(ctx: &CoreContext, blobrepo: &BlobRepo, head: ChangesetId) -> Result<()> {
    let phases = blobrepo.get_phases();
    let sql_phases = phases.get_sql_phases();
    mark_reachable_as_public(&ctx, sql_phases, &[head], false).await?;
    Ok(())
}

#[fbinit::compat_test]
async fn test_iddag_save_store(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = linear::getrepo(fb).await;
    let repo_id = blobrepo.get_repoid();
    let mut dag = Dag::new_in_process(repo_id)?;

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

    let blobstore = memblob::LazyMemblob::new();
    let iddag_save_store = IdDagSaveStore::new(repo_id, Arc::new(blobstore));
    iddag_save_store
        .save(&ctx, IdDagVersion(1), &dag.iddag)
        .await?;

    assert!(
        iddag_save_store
            .find(&ctx, IdDagVersion(2))
            .await?
            .is_none()
    );
    let loaded_id_dag = iddag_save_store.load(&ctx, IdDagVersion(1)).await?;
    let from_save = Dag::new(repo_id, loaded_id_dag, dag.idmap.clone());
    let answer = from_save
        .location_to_changeset_id(&ctx, known_cs, distance)
        .await?;
    assert_eq!(answer, expected_cs);

    Ok(())
}
