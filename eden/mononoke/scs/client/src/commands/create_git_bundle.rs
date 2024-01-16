/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::bail;
use anyhow::Result;
use serde::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::repo::RepoArgs;
use crate::args::service_id::ServiceIdArgs;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]

/// Generate a Git bundle for a stack of commits
///
/// Provide two commits: the first is the head of a stack, and the second is
/// public commit the stack is based on.  The stack of commits between these
/// two commits will be included in the Git bundle.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
    #[clap(flatten)]
    service_id_args: ServiceIdArgs,
}

#[derive(Serialize)]
struct RepoStackGitBundleStoreOutput {
    everstore_handle: String,
}

impl Render for RepoStackGitBundleStoreOutput {
    type Args = ();

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        writeln!(
            w,
            "Everstore handle for git bundle: {}",
            self.everstore_handle,
        )?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.into_repo_specifier();
    let commit_ids = args.commit_ids_args.into_commit_ids();
    if commit_ids.len() != 2 {
        bail!("expected 2 commit_ids (got {})", commit_ids.len())
    }
    let conn = app.get_connection(Some(&repo.name))?;
    let ids = resolve_commit_ids(&conn, &repo, &commit_ids).await?;
    let service_identity = args.service_id_args.service_id;

    let (head, base) = match ids.as_slice() {
        [head_id, base_id] => (head_id.clone(), base_id.clone()),
        _ => bail!("expected 1 or 2 commit_ids (got {})", ids.len()),
    };

    let params = thrift::RepoStackGitBundleStoreParams {
        head,
        base,
        service_identity,
        ..Default::default()
    };
    let outcome = conn.repo_stack_git_bundle_store(&repo, &params).await?;
    let output = RepoStackGitBundleStoreOutput {
        everstore_handle: outcome.everstore_handle,
    };
    app.target.render_one(&(), output).await
}
