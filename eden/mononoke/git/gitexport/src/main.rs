/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Error;
use anyhow::Result;
use bookmarks_types::BookmarkKey;
use fbinit::FacebookInit;
use gitexport_tools::build_partial_commit_graph_for_export;
use gitexport_tools::rewrite_partial_changesets;
use mononoke_api::BookmarkFreshness;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_app::args::RepoArg;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_types::MPath;
use repo_authorization::AuthorizationContext;
use slog::debug;

use crate::types::GitExportArgs;

pub mod types {
    use std::path::PathBuf;

    use clap::Parser;
    use mononoke_app::args::RepoArgs;

    /// Mononoke Git Exporter
    #[derive(Debug, Parser)]
    pub struct GitExportArgs {
        /// Name of the hg repo being exported
        #[clap(flatten)]
        pub hg_repo_args: RepoArgs,

        // TODO(T160787114): programatically create a temp repo
        #[clap(long)]
        pub temp_repo_name: String,

        /// Path to the git repo being created
        #[clap(long)]
        pub output: Option<PathBuf>, // TODO(T160787114): Make this required

        /// List of directories in `hg_repo` to be exported to a git repo
        #[clap(long, short('p'))]
        /// Paths in the source hg repo that should be exported to a git repo.
        pub export_paths: Vec<PathBuf>,
        // TODO(T160600443): support last revision argument
        // TODO(T160600443): support until_timestamp argument
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app: MononokeApp = MononokeAppBuilder::new(fb)
        .with_app_extension(Fb303AppExtension {})
        .build::<GitExportArgs>()?;

    app.run_with_monitoring_and_logging(async_main, "gitexport", AliveService)
}

async fn async_main(app: MononokeApp) -> Result<(), Error> {
    let args: GitExportArgs = app.args()?;
    let logger = app.logger();
    let ctx = app.new_basic_context();

    let repo = app.open_repo(&args.hg_repo_args).await?;

    let auth_ctx = AuthorizationContext::new_bypass_access_control();
    let repo_ctx: RepoContext = RepoContext::new(ctx, auth_ctx.into(), repo, None, None).await?;

    // TODO(T160600443): support using a specific changeset as starting commit,
    // instead of a bookmark.
    let bookmark_key = BookmarkKey::from_str("master")?;

    let cs_ctx = repo_ctx
        .resolve_bookmark(&bookmark_key, BookmarkFreshness::MostRecent)
        .await?
        .unwrap();

    let export_paths = args
        .export_paths
        .into_iter()
        .map(|p| TryFrom::try_from(p.as_os_str()))
        .collect::<Result<Vec<MPath>>>()?;

    let (changesets, cs_parents) =
        build_partial_commit_graph_for_export(logger, export_paths.clone(), cs_ctx).await?;

    debug!(logger, "changesets: {:#?}", changesets);
    debug!(logger, "changeset parents: {:?}", cs_parents);

    let temp_repo_args = RepoArg::Name(args.temp_repo_name);
    let temp_repo: Repo = app.open_repo(&temp_repo_args).await?;

    let auth_ctx = AuthorizationContext::new_bypass_access_control();
    let target_repo_ctx: RepoContext = RepoContext::new(
        repo_ctx.ctx().clone(),
        auth_ctx.into(),
        temp_repo.into(),
        None,
        None,
    )
    .await?;

    rewrite_partial_changesets(
        repo_ctx,
        changesets,
        &cs_parents,
        &target_repo_ctx,
        export_paths,
    )
    .await?;

    // TODO(T160787114): export temporary repo to git repo

    Ok(())
}
