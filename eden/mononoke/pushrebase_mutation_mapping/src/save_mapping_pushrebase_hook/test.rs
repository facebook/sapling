/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use borrowed::borrowed;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::hashset;
use metaconfig_types::PushrebaseFlags;
use mononoke_types_mocks::repo;
use pushrebase::do_pushrebase_bonsai;
use test_repo_factory::TestRepoFactory;
use tests_utils::bookmark;
use tests_utils::CreateCommitContext;

use super::SaveMappingPushrebaseHook;
use crate::get_prepushrebase_ids;

#[fbinit::test]
async fn pushrebase_saves_mapping(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let mut repo_factory = TestRepoFactory::new(fb)?;
    let repo: BlobRepo = repo_factory.with_id(repo::REPO_ONE).build()?;

    borrowed!(ctx, repo);

    let root = CreateCommitContext::new_root(ctx, repo).commit().await?;

    let master = bookmark(ctx, repo, "master").set_to(root).await?;
    let main = bookmark(ctx, repo, "main").set_to(root).await?;

    let cs = CreateCommitContext::new(ctx, repo, vec![root])
        .commit()
        .await?
        .load(ctx, repo.blobstore())
        .await?;

    let hooks = [SaveMappingPushrebaseHook::new(repo.get_repoid())];

    // Pushrebase the same commit onto different bookmarks that are pointing to
    // the same commit (root).
    do_pushrebase_bonsai(
        ctx,
        repo,
        &PushrebaseFlags {
            rewritedates: false,
            ..Default::default()
        },
        &master,
        &hashset![cs.clone()],
        &hooks,
    )
    .await?;

    let rebased = do_pushrebase_bonsai(
        ctx,
        repo,
        &PushrebaseFlags {
            rewritedates: false,
            ..Default::default()
        },
        &main,
        &hashset![cs.clone()],
        &hooks,
    )
    .await?
    .head;

    let prepushrebase_ids = get_prepushrebase_ids(
        &repo_factory.metadata_db().connections().read_connection,
        repo.get_repoid(),
        rebased,
    )
    .await?;

    assert_eq!(
        prepushrebase_ids,
        vec![cs.get_changeset_id(), cs.get_changeset_id()]
    );

    Ok(())
}
