/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use scs_client_raw::thrift;
use serde::Serialize;
use source_control_clients::errors::CommitSparseProfileSizePollError;

use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::repo::RepoArgs;
use crate::args::sparse_profiles::SparseProfilesArgs;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Calculate the total size of each sparse profile for a given commit
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(flatten)]
    commit_id_args: CommitIdArgs,

    #[clap(flatten)]
    sparse_profiles_args: SparseProfilesArgs,

    #[clap(long = "async")]
    asynchronous: bool,
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

    let response = if args.asynchronous {
        let params = thrift::CommitSparseProfileSizeParamsV2 {
            commit: commit.clone(),
            profiles,
            ..Default::default()
        };
        let token = conn.commit_sparse_profile_size_async(&params).await?;

        loop {
            let res = conn.commit_sparse_profile_size_poll(&token).await;
            match res {
                Ok(res) => match res {
                    source_control::CommitSparseProfileSizePollResponse::response(success) => {
                        break success;
                    }
                    source_control::CommitSparseProfileSizePollResponse::poll_pending(_) => {
                        println!("sparse profile size is not ready yet, waiting some more...");
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
                        // retry
                    }
                    _ => return Err(anyhow::anyhow!("request failed with error: {:?}", e)),
                },
            }
        }
    } else {
        let params = thrift::CommitSparseProfileSizeParams {
            profiles,
            ..Default::default()
        };
        conn.commit_sparse_profile_size(&commit, &params).await?
    };

    let output = SparseProfileSizeOutput {
        profiles_size: response.profiles_size,
    };

    app.target.render_one(&args, output).await
}
