/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use anyhow::bail;
use anyhow::format_err;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use cloned::cloned;
use context::CoreContext;
use futures::try_join;
use megarepolib::common::ChangesetArgs as MegarepoNewChangesetArgs;
use megarepolib::common::create_save_and_generate_hg_changeset;
use megarepolib::working_copy::get_colliding_paths_between_commits;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use mononoke_types::ChangesetId;
use slog::info;

use super::common::ResultingChangesetArgs;

/// Create a merge commit with given parents
#[derive(Debug, clap::Args)]
pub struct MergeArgs {
    #[clap(flatten, help = "first and second parents of a produced merge commit")]
    pub parents: ChangesetArgs,
    #[clap(flatten)]
    pub repo_args: RepoArgs,

    #[command(flatten)]
    pub res_cs_args: ResultingChangesetArgs,
}

async fn fail_on_path_conflicts(
    ctx: &CoreContext,
    repo: &Repo,
    hg_cs_id_1: HgChangesetId,
    hg_cs_id_2: HgChangesetId,
) -> Result<(), Error> {
    info!(ctx.logger(), "Checking if there are any path conflicts");
    let (bcs_1, bcs_2) = try_join!(
        repo.bonsai_hg_mapping().get_bonsai_from_hg(ctx, hg_cs_id_1),
        repo.bonsai_hg_mapping().get_bonsai_from_hg(ctx, hg_cs_id_2)
    )?;
    let collisions =
        get_colliding_paths_between_commits(ctx, repo, bcs_1.unwrap(), bcs_2.unwrap()).await?;
    if !collisions.is_empty() {
        Err(format_err!(
            "There are paths present in both parents: {:?} ...",
            collisions.iter().take(10).collect::<Vec<_>>(),
        ))
    } else {
        info!(ctx.logger(), "Done checking path conflicts");
        Ok(())
    }
}

pub async fn perform_merge(
    ctx: CoreContext,
    repo: Repo,
    first_bcs_id: ChangesetId,
    second_bcs_id: ChangesetId,
    res_cs_args: MegarepoNewChangesetArgs,
) -> Result<HgChangesetId, Error> {
    cloned!(ctx, repo);
    let (first_hg_cs_id, second_hg_cs_id) = try_join!(
        repo.derive_hg_changeset(&ctx, first_bcs_id.clone()),
        repo.derive_hg_changeset(&ctx, second_bcs_id.clone()),
    )?;
    fail_on_path_conflicts(&ctx, &repo, first_hg_cs_id, second_hg_cs_id).await?;
    info!(
        ctx.logger(),
        "Creating a merge bonsai changeset with parents: {:?}, {:?}", &first_bcs_id, &second_bcs_id
    );
    create_save_and_generate_hg_changeset(
        &ctx,
        &repo,
        vec![first_bcs_id, second_bcs_id],
        Default::default(),
        res_cs_args,
    )
    .await
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: MergeArgs) -> Result<()> {
    info!(ctx.logger(), "Creating a merge commit");

    let repo: Repo = app.open_repo(&args.repo_args).await?;

    let parents = args.parents.resolve_changesets(ctx, &repo).await?;
    let (first_parent, second_parent) = match parents[..] {
        [first_parent, second_parent] => (first_parent, second_parent),
        _ => bail!("Expected exactly two parent commits"),
    };

    let res_cs_args = args.res_cs_args.try_into()?;

    perform_merge(
        ctx.clone(),
        repo.clone(),
        first_parent,
        second_parent,
        res_cs_args,
    )
    .await
    .map(|_| ())
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use fbinit::FacebookInit;
    use fixtures::MergeEven;
    use fixtures::TestRepoFixture;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_path_conflict_detection(fb: FacebookInit) {
        let repo = MergeEven::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);
        let p1 = HgChangesetId::from_str("4f7f3fd428bec1a48f9314414b063c706d9c1aed").unwrap();
        let p2 = HgChangesetId::from_str("16839021e338500b3cf7c9b871c8a07351697d68").unwrap();
        assert!(
            fail_on_path_conflicts(&ctx, &repo, p1, p2).await.is_err(),
            "path conflicts should've been detected"
        );
    }
}
