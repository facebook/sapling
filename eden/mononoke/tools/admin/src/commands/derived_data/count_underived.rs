/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use cloned::cloned;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_app::args::ChangesetArgs;
use repo_derived_data::RepoDerivedDataRef;

use super::args::DerivedUtilsArgs;
use super::Repo;

#[derive(Args)]
pub(super) struct CountUnderivedArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
    #[clap(flatten)]
    derived_utils_args: DerivedUtilsArgs,
}

pub(super) async fn count_underived(
    ctx: &CoreContext,
    repo: &Repo,
    args: CountUnderivedArgs,
) -> Result<()> {
    let derived_utils = args.derived_utils_args.derived_utils(ctx, repo)?;

    let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;

    stream::iter(cs_ids)
        .map(|cs_id| {
            cloned!(derived_utils);
            async move {
                let underived = derived_utils
                    .count_underived(ctx, repo.repo_derived_data(), cs_id)
                    .await?;
                Result::<_>::Ok((cs_id, underived))
            }
        })
        .buffer_unordered(10)
        .try_for_each(|(cs_id, underived)| async move {
            println!("{}: {}", cs_id, underived);
            Ok(())
        })
        .await
}
