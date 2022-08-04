/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use serde::Serialize;
use source_control::types as thrift;

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
    let commit_id = resolve_commit_id(&app.connection, &repo, &commit_id).await?;

    let profiles = args.sparse_profiles_args.clone().into_sparse_profiles();

    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id: commit_id,
        ..Default::default()
    };

    let params = thrift::CommitSparseProfileSizeParams {
        profiles,
        ..Default::default()
    };

    let response = app
        .connection
        .commit_sparse_profile_size(&commit, &params)
        .await?;

    let output = SparseProfileSizeOutput {
        profiles_size: response.profiles_size,
    };

    app.target.render_one(&args, output).await
}
