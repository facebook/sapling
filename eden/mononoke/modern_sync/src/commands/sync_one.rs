/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use clap::Parser;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::SessionContainer;
use metadata::Metadata;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use repo_identity::RepoIdentityRef;
use url::Url;

use crate::sender::dummy::DummySender;
use crate::sender::edenapi::EdenapiSender;
use crate::sender::ModernSyncSender;
use crate::ModernSyncArgs;
use crate::Repo;

/// Sync one changeset (debug only)
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(long, help = "Changeset to sync")]
    cs_id: ChangesetId,
    #[clap(long, help = "Print sent items without actually syncing")]
    dry_run: bool,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let app_args = &app.args::<ModernSyncArgs>()?;
    let repo: Repo = app.open_repo(&app_args.repo).await?;
    let _repo_id = repo.repo_identity().id();
    let repo_name = repo.repo_identity().name().to_string();

    let config = repo
        .repo_config
        .modern_sync_config
        .clone()
        .ok_or(format_err!(
            "No modern sync config found for repo {}",
            repo_name
        ))?;
    let logger = app.logger().clone();

    let mut metadata = Metadata::default();
    metadata.add_client_info(ClientInfo::default_with_entry_point(
        ClientEntryPoint::ModernSync,
    ));

    let mut scuba = app.environment().scuba_sample_builder.clone();
    scuba.add_metadata(&metadata);

    let session_container = SessionContainer::builder(app.fb)
        .metadata(Arc::new(metadata))
        .build();

    let ctx = session_container
        .new_context(app.logger().clone(), scuba)
        .clone_with_repo_name(&repo_name.clone());

    let sender: Arc<dyn ModernSyncSender + Send + Sync> = if args.dry_run {
        Arc::new(DummySender::new(logger.clone()))
    } else {
        let url = if let Some(socket) = app_args.dest_socket {
            // Only for integration tests
            format!("{}:{}/edenapi/", &config.url, socket)
        } else {
            format!("{}/edenapi/", &config.url)
        };

        let tls_args = app_args
            .tls_params
            .clone()
            .ok_or_else(|| format_err!("TLS params not found for repo {}", repo_name))?;

        let dest_repo = app_args.dest_repo_name.clone().unwrap_or(repo_name.clone());

        Arc::new(EdenapiSender::new(Url::parse(&url)?, dest_repo, logger.clone(), tls_args).await?)
    };

    crate::sync::process_one_changeset(&args.cs_id, &ctx, repo, &logger, sender, false).await?;

    Ok(())
}
