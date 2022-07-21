/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use borrowed::borrowed;
use context::CoreContext;
use fbinit::FacebookInit;
use maplit::hashset;
use mononoke_types::RepositoryId;
use mononoke_types_mocks::hash::*;
use pushrebase::do_pushrebase_bonsai;
use test_repo_factory::TestRepoFactory;
use tests_utils::bookmark;
use tests_utils::CreateCommitContext;

use crate::GitMappingPushrebaseHook;

#[fbinit::test]
fn pushrebase_populates_git_mapping(fb: FacebookInit) -> Result<(), Error> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(pushrebase_populates_git_mapping_impl(fb))
}

async fn pushrebase_populates_git_mapping_impl(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BlobRepo = TestRepoFactory::new(fb)?
        .with_id(RepositoryId::new(1))
        .build()?;
    borrowed!(ctx, repo);
    let mapping = repo.bonsai_git_mapping().clone();

    let root = CreateCommitContext::new_root(ctx, repo).commit().await?;

    let cs1 = CreateCommitContext::new(ctx, repo, vec![root])
        .commit()
        .await?;

    let cs2 = CreateCommitContext::new(ctx, repo, vec![root])
        .add_extra("hg-git-rename-source".to_owned(), b"git".to_vec())
        .add_extra(
            "convert_revision".to_owned(),
            TWOS_GIT_SHA1.to_hex().as_bytes().to_owned(),
        )
        .commit()
        .await?
        .load(ctx, repo.blobstore())
        .await?;

    let book = bookmark(ctx, repo, "master").set_to(cs1).await?;

    let hooks = [GitMappingPushrebaseHook::new(
        repo.bonsai_git_mapping().clone(),
    )];

    let rebased = do_pushrebase_bonsai(
        ctx,
        repo,
        &Default::default(),
        &book,
        &hashset![cs2.clone()],
        &hooks,
    )
    .await?
    .rebased_changesets;

    let cs2_rebased = rebased
        .iter()
        .find(|e| e.id_old == cs2.get_changeset_id())
        .ok_or_else(|| Error::msg("missing cs2"))?
        .id_new
        .load(ctx, repo.blobstore())
        .await?;

    let cs3 = CreateCommitContext::new(ctx, repo, vec![root])
        .add_extra("hg-git-rename-source".to_owned(), b"git".to_vec())
        .add_extra(
            "convert_revision".to_owned(),
            THREES_GIT_SHA1.to_hex().as_bytes().to_owned(),
        )
        .commit()
        .await?
        .load(ctx, repo.blobstore())
        .await?;

    let cs4 = CreateCommitContext::new(ctx, repo, vec![cs3.get_changeset_id()])
        .add_extra("hg-git-rename-source".to_owned(), b"git".to_vec())
        .add_extra(
            "convert_revision".to_owned(),
            FOURS_GIT_SHA1.to_hex().as_bytes().to_owned(),
        )
        .commit()
        .await?
        .load(ctx, repo.blobstore())
        .await?;

    let rebased = do_pushrebase_bonsai(
        ctx,
        repo,
        &Default::default(),
        &book,
        &hashset![cs3.clone(), cs4.clone()],
        &hooks,
    )
    .await?
    .rebased_changesets;

    let cs3_rebased = rebased
        .iter()
        .find(|e| e.id_old == cs3.get_changeset_id())
        .ok_or_else(|| Error::msg("missing cs3"))?
        .id_new
        .load(ctx, repo.blobstore())
        .await?;

    let cs4_rebased = rebased
        .iter()
        .find(|e| e.id_old == cs4.get_changeset_id())
        .ok_or_else(|| Error::msg("missing cs4"))?
        .id_new
        .load(ctx, repo.blobstore())
        .await?;

    assert_eq!(
        Some(TWOS_GIT_SHA1),
        mapping
            .get_git_sha1_from_bonsai(ctx, cs2_rebased.get_changeset_id())
            .await?,
    );
    assert_eq!(
        Some(THREES_GIT_SHA1),
        mapping
            .get_git_sha1_from_bonsai(ctx, cs3_rebased.get_changeset_id())
            .await?,
    );
    assert_eq!(
        Some(FOURS_GIT_SHA1),
        mapping
            .get_git_sha1_from_bonsai(ctx, cs4_rebased.get_changeset_id())
            .await?,
    );

    Ok(())
}
