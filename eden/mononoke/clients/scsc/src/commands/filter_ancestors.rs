/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Filter a list of candidate commits to only those that are ancestors of a target commit.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use anyhow::bail;
use commit_id_types::CommitIdArgs;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::SchemeArgs;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::library::commit_id::render_commit_id;
use crate::render::Render;

#[derive(clap::Parser)]
/// Filters a list of candidate commits to only those that are ancestors of the target commit.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(long, num_args = 1.., required = true)]
    /// Candidate commit IDs to check for ancestry
    candidates: Vec<String>,
}

#[derive(Serialize)]
struct FilterAncestorsOutput {
    ancestors: Vec<BTreeMap<String, String>>,
}

impl Render for FilterAncestorsOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        let schemes = args.scheme_args.scheme_string_set();
        for ids in &self.ancestors {
            render_commit_id(None, "\n", "ancestor", ids, &schemes, w)?;
            write!(w, "\n")?;
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let conn = app.get_connection(Some(&repo.name)).await?;

    // Resolve the target (descendant) commit
    let descendant_id = args.commit_id_args.clone().into_commit_id();
    let descendant_resolved = resolve_commit_id(&conn, &repo, &descendant_id).await?;

    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id: descendant_resolved,
        ..Default::default()
    };

    // Resolve candidate commit IDs
    let candidate_commit_ids: Vec<commit_id_types::CommitId> = args
        .candidates
        .iter()
        .map(|c| commit_id_types::CommitId::Resolve(c.clone()))
        .collect();

    if candidate_commit_ids.is_empty() {
        bail!("expected at least one candidate commit id");
    }

    let resolved_candidates = resolve_commit_ids(&conn, &repo, candidate_commit_ids.iter()).await?;

    let params = thrift::CommitFilterAncestorsParams {
        candidate_ancestor_ids: resolved_candidates,
        identity_schemes: args.scheme_args.clone().into_request_schemes(),
        ..Default::default()
    };

    let response = conn
        .commit_filter_ancestors(&commit, &params)
        .await
        .map_err(|e| e.handle_selection_error(&commit.repo))?;

    let ancestors: Vec<BTreeMap<String, String>> = response
        .ancestors
        .iter()
        .map(
            |ids: &BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>| {
                map_commit_ids(ids.values())
            },
        )
        .collect();

    let output = FilterAncestorsOutput { ancestors };
    app.target.render_one(&args, output).await
}
