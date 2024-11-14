/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::bail;
use anyhow::Result;
use scs_client_raw::thrift;
use serde::Serialize;
use source_control_clients::errors::CommitSparseProfileDeltaPollError;

use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::repo::RepoArgs;
use crate::args::sparse_profiles::SparseProfilesArgs;
use crate::render::Render;
use crate::ScscApp;

const POLL_SLEEP_DURATION: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(clap::Parser)]
/// Calculate the size change for each sparse profile between two given commits
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,

    #[clap(flatten)]
    sparse_profiles_args: SparseProfilesArgs,

    #[clap(long = "async")]
    asynchronous: bool,
}

#[derive(Serialize)]
struct SparseProfileDeltaOutput {
    changed_sparse_profiles: Option<thrift::SparseProfileDeltaSizes>,
}

impl Render for SparseProfileDeltaOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if let Some(delta_sizes) = &self.changed_sparse_profiles {
            if !delta_sizes.size_changes.is_empty() {
                for (profile_name, profile_change) in delta_sizes.size_changes.iter() {
                    match profile_change.change {
                        thrift::SparseProfileChangeElement::added(thrift::SparseProfileAdded {
                            size,
                            ..
                        }) => writeln!(w, "profile {} was added, size: {}", profile_name, size)?,
                        thrift::SparseProfileChangeElement::removed(
                            thrift::SparseProfileRemoved { previous_size, .. },
                        ) => writeln!(
                            w,
                            "profile {} was removed, previous size: {}",
                            profile_name, previous_size
                        )?,
                        thrift::SparseProfileChangeElement::changed(
                            thrift::SparseProfileSizeChanged { size_change, .. },
                        ) => writeln!(
                            w,
                            "profile {} was changed, size change: {}",
                            profile_name, size_change
                        )?,
                        _ => bail!("unrecognized change!"),
                    };
                }
            } else {
                writeln!(w, "no changes found")?;
            }
        } else {
            writeln!(w, "no changes found")?;
        }

        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();

    let commit_ids = args.commit_ids_args.clone().into_commit_ids();
    if commit_ids.len() != 2 {
        bail!("expected 2 commit_ids (got {})", commit_ids.len())
    }

    let conn = app.get_connection(Some(&repo.name))?;
    let commit_ids = resolve_commit_ids(&conn, &repo, &commit_ids).await?;

    let profiles = args.sparse_profiles_args.clone().into_sparse_profiles();

    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id: commit_ids[0].clone(),
        ..Default::default()
    };

    let response = if args.asynchronous {
        let params = thrift::CommitSparseProfileDeltaParamsV2 {
            commit: commit.clone(),
            other_id: commit_ids[1].clone(),
            profiles,
            ..Default::default()
        };
        let token = conn.commit_sparse_profile_delta_async(&params).await?;

        loop {
            // reopening the connection on retry might allow SR to send us to a different server
            let conn = app.get_connection(Some(&repo.name))?;
            let res = conn.commit_sparse_profile_delta_poll(&token).await;
            match res {
                Ok(res) => match res {
                    source_control::CommitSparseProfileDeltaPollResponse::response(success) => {
                        break success;
                    }
                    source_control::CommitSparseProfileDeltaPollResponse::poll_pending(_) => {
                        eprintln!("sparse profile size is not ready yet, waiting some more...");
                    }
                    source_control::CommitSparseProfileDeltaPollResponse::UnknownField(t) => {
                        return Err(anyhow::anyhow!(
                            "request failed with unknown result: {:?}",
                            t
                        ));
                    }
                },
                Err(e) => match e {
                    CommitSparseProfileDeltaPollError::poll_error(_) => {
                        eprintln!("poll error, retrying...");
                    }
                    _ => return Err(anyhow::anyhow!("request failed with error: {:?}", e)),
                },
            }
            tokio::time::sleep(POLL_SLEEP_DURATION).await;
        }
    } else {
        let params = thrift::CommitSparseProfileDeltaParams {
            other_id: commit_ids[1].clone(),
            profiles,
            ..Default::default()
        };
        conn.commit_sparse_profile_delta(&commit, &params).await?
    };

    let output = SparseProfileDeltaOutput {
        changed_sparse_profiles: response.changed_sparse_profiles,
    };

    app.target.render_one(&args, output).await
}
