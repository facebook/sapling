/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Find common base of two commits

use std::io::Write;

use anyhow::bail;
use anyhow::Result;
use serde::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::repo::RepoArgs;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Finds whether the first provided commit is an ancestor of the second one.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
}

#[derive(Serialize)]
struct IsAncestorOutput {
    result: bool,
}

impl Render for IsAncestorOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        writeln!(w, "{:?}", self.result)?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_ids = args.commit_ids_args.clone().into_commit_ids();
    let ids = resolve_commit_ids(&app.connection, &repo, &commit_ids).await?;
    if ids.len() != 2 {
        bail!("expected 2 commit_ids (got {})", commit_ids.len())
    }
    let commit = thrift::CommitSpecifier {
        repo,
        id: ids[0].clone(),
        ..Default::default()
    };
    let params = thrift::CommitIsAncestorOfParams {
        descendant_commit_id: ids[1].clone(),
        ..Default::default()
    };
    let response = app
        .connection
        .commit_is_ancestor_of(&commit, &params)
        .await?;
    let output = IsAncestorOutput { result: response };
    app.target.render_one(&args, output).await
}
