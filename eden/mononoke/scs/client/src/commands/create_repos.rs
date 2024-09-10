/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use scs_client_raw::thrift;

use crate::ScscApp;

#[derive(clap::Parser)]
/// Create a bookmark
pub(super) struct CommandArgs {
    /// Dry run
    #[clap(long, short = 'n')]
    dry_run: bool,
    /// Hipster group to use for newly created ACL (if not specified, will not create new ACL)
    #[clap(long)]
    hipster_group: Option<String>,
    /// Oncall owning the repo
    #[clap(long)]
    oncall_name: String,
    /// Names of the repos to create
    repo_names: Vec<String>,
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let conn = app.get_connection(None)?;
    let repos = args
        .repo_names
        .into_iter()
        .map(|repo_name| thrift::RepoCreationRequest {
            repo_name,
            scm_type: thrift::RepoScmType::GIT,
            oncall_name: args.oncall_name.clone(),
            size_bucket: thrift::RepoSizeBucket::SMALL,
            ..Default::default()
        })
        .collect();
    let params = thrift::CreateReposParams {
        repos,
        dry_run: args.dry_run,
        ..Default::default()
    };
    let token = conn.create_repos(&params).await?;

    // Repo creation is potentially asynchronous request. Let's poll it until it's done.
    loop {
        let res = conn.create_repos_poll(&token).await?;
        if res.result.is_some() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }
    Ok(())
}
