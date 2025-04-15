/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use blobstore_factory::MetadataSqlFactory;
use bookmarks::BookmarkKey;
use context::CoreContext;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use megarepolib::common::ChangesetArgs as MegarepoNewChangesetArgs;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::AsRepoArg;
use mononoke_app::args::RepoArgs;
use mononoke_types::DateTime;
use pushredirect::SqlPushRedirectionConfigBuilder;
use sql_query_config::SqlQueryConfigArc;

#[derive(Debug, clap::Args, Clone)]
pub(crate) struct ResultingChangesetArgs {
    #[clap(long, short = 'm')]
    pub commit_message: String,
    #[clap(long, short = 'a')]
    pub commit_author: String,

    #[clap(long = "commit-date-rfc3339")]
    pub datetime: Option<String>,

    #[clap(
        long,
        help = "bookmark to point to resulting commits (no sanity checks, will move existing bookmark, be careful)"
    )]
    pub set_bookmark: Option<String>,

    #[clap(long = "mark-public")]
    pub mark_public: bool,
}

impl TryInto<MegarepoNewChangesetArgs> for ResultingChangesetArgs {
    type Error = Error;

    fn try_into(self) -> Result<MegarepoNewChangesetArgs> {
        let mb_datetime = self
            .datetime
            .as_deref()
            .map_or_else(|| Ok(DateTime::now()), DateTime::from_rfc3339)?;

        let mb_bookmark = self.set_bookmark.map(BookmarkKey::new).transpose()?;
        let res = MegarepoNewChangesetArgs {
            message: self.commit_message,
            author: self.commit_author,
            datetime: mb_datetime,
            bookmark: mb_bookmark,
            mark_public: self.mark_public,
        };
        Ok(res)
    }
}

pub(crate) async fn get_live_commit_sync_config(
    _ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: RepoArgs,
) -> Result<Arc<CfgrLiveCommitSyncConfig>> {
    let config_store = app.environment().config_store.clone();
    let repo: Arc<Repo> = app.open_repo_unredacted(&repo_args).await?;
    let (_, repo_config) = app.repo_config(repo_args.as_repo_arg())?;
    let sql_factory: MetadataSqlFactory = MetadataSqlFactory::new(
        app.fb,
        repo_config.storage_config.metadata.clone(),
        app.mysql_options().clone(),
        *app.readonly_storage(),
    )
    .await?;
    let builder = sql_factory
        .open::<SqlPushRedirectionConfigBuilder>()
        .await?;
    let push_redirection_config = builder.build(repo.sql_query_config_arc());
    let live_commit_sync_config = Arc::new(CfgrLiveCommitSyncConfig::new(
        &config_store,
        Arc::new(push_redirection_config),
    )?);

    Ok(live_commit_sync_config)
}
