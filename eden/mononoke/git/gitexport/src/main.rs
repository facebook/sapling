/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Error;
use bookmarks_types::BookmarkKey;
use fbinit::FacebookInit;
use gitexport_tools::build_partial_commit_graph_for_export;
pub use mononoke_api::BookmarkFreshness;
use mononoke_api::RepoContext;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use repo_authorization::AuthorizationContext;

use crate::types::GitExportArgs;

pub mod types {
    use clap::Parser;
    use mononoke_app::args::RepoArgs;

    /// Mononoke Git Exporter
    #[derive(Debug, Parser)]
    pub struct GitExportArgs {
        /// Name of the hg repo being exported
        #[clap(flatten)]
        pub hg_repo_args: RepoArgs,

        /// Path to the git repo being created
        #[clap(long)]
        pub output: Option<String>, // TODO(T160787114): Make this required

        /// List of directories in `hg_repo` to be exported to a git repo
        #[clap(long)]
        // TODO(T161204758): change this to a Vec<String> when we can support multiple export paths
        pub export_path: String,
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
    let ctx = app.new_basic_context();

    let repo = app.open_repo(&args.hg_repo_args).await?;

    let auth_ctx = AuthorizationContext::new_bypass_access_control();
    let repo_ctx: RepoContext = RepoContext::new(ctx, auth_ctx.into(), repo, None, None).await?;

    // TODO(T160600443): support using a specific changeset as starting commit,
    // instead of a bookmark.
    let bookmark_key = BookmarkKey::from_str("master")?;

    let chgset_ctx = repo_ctx
        .resolve_bookmark(&bookmark_key, BookmarkFreshness::MostRecent)
        .await?
        .unwrap();

    // TODO(T161204758): get multiple paths from arguments once we sort and
    // dedupe changesets properly and can build a commit graph from multiple
    // changeset lists
    let export_paths = vec![args.export_path.as_str()];

    let _commit_graph = build_partial_commit_graph_for_export(export_paths, chgset_ctx).await;

    // TODO(T160787114): export commit graph to temporary mononore repo

    // TODO(T160787114): export temporary repo to git repo

    Ok(())
}
