/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use pushredirect::PushRedirectionConfig;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use slog::info;

#[derive(Args)]
pub(super) struct ShowArgs {
    #[clap(flatten)]
    repo: RepoArgs,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    pub push_redirection_config: dyn PushRedirectionConfig,
}

pub(super) async fn show(ctx: &CoreContext, app: MononokeApp, args: ShowArgs) -> Result<()> {
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;
    let repo_id = repo.repo_identity().id();

    match repo.push_redirection_config.get(ctx, repo_id).await? {
        Some(res) => {
            info!(
                ctx.logger(),
                "{}: draft={} public={}", res.repo_id, res.draft_push, res.public_push,
            );
        }
        None => {
            info!(
                ctx.logger(),
                "{}: not in the db, default draft=false public=false", repo_id,
            );
        }
    }

    Ok(())
}
