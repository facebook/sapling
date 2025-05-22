/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use commit_id_types::CommitIdArgs;
use scs_client_raw::thrift;
use serde::Serialize;
use source_control_clients::errors::CommitSparseProfileSizePollError;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::repo::RepoArgs;
use crate::args::sparse_profiles::SparseProfilesArgs;
use crate::render::Render;

const POLL_SLEEP_DURATION: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(clap::Parser)]
/// Calculate the total size of each sparse profile for a given commit
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(flatten)]
    commit_id_args: CommitIdArgs,

    #[clap(flatten)]
    sparse_profiles_args: SparseProfilesArgs,
}

#[derive(Serialize)]
struct SparseProfileSizeOutput {
    profiles_size: thrift::SparseProfileSizes,
}

impl Render for SparseProfileSizeOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if self.profiles_size.sizes.is_empty() {
            writeln!(w, "no profiles to display")?;
        } else {
            for (profile_name, thrift::SparseProfileSize { size, .. }) in
                self.profiles_size.sizes.iter()
            {
                writeln!(w, "profile: {}, size: {}", profile_name, size)?;
            }
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();

    let commit_id = args.commit_id_args.clone().into_commit_id();
    let conn = app.get_connection(Some(&repo.name))?;
    let commit_id = resolve_commit_id(&conn, &repo, &commit_id).await?;

    let profiles = args.sparse_profiles_args.clone().into_sparse_profiles();

    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id: commit_id,
        ..Default::default()
    };

    let params = thrift::CommitSparseProfileSizeParamsV2 {
        commit: commit.clone(),
        profiles,
        ..Default::default()
    };
    let token = conn.commit_sparse_profile_size_async(&params).await?;

    let now = std::time::Instant::now();
    let response = loop {
        if now.elapsed() > std::time::Duration::from_secs(600) {
            return Err(anyhow::anyhow!("request timed out"));
        }

        // reopening the connection on retry might allow SR to send us to a different server
        let conn = app.get_connection(Some(&repo.name))?;
        let res = conn.commit_sparse_profile_size_poll(&token).await;
        match res {
            Ok(res) => match res {
                source_control::CommitSparseProfileSizePollResponse::response(success) => {
                    break success;
                }
                source_control::CommitSparseProfileSizePollResponse::poll_pending(_) => {
                    eprintln!("sparse profile size is not ready yet, waiting some more...");
                }
                source_control::CommitSparseProfileSizePollResponse::UnknownField(t) => {
                    return Err(anyhow::anyhow!(
                        "request failed with unknown result: {:?}",
                        t
                    ));
                }
            },
            Err(e) => match e {
                CommitSparseProfileSizePollError::poll_error(_) => {
                    eprintln!("poll error, retrying...");
                }
                _ => return Err(anyhow::anyhow!("request failed with error: {:?}", e)),
            },
        }
        tokio::time::sleep(POLL_SLEEP_DURATION).await;
    };

    let output = SparseProfileSizeOutput {
        profiles_size: response.profiles_size,
    };

    app.target.render_one(&args, output).await
}
