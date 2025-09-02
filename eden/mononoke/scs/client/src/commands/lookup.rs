/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Look up a bookmark or commit id.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::Write;

use anyhow::Result;
use anyhow::bail;
use commit_id_types::CommitId;
use commit_id_types::CommitIdsArgs;
use scs_client_raw::ScsClient;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::SchemeArgs;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::library::commit_id::render_commit_id;
use crate::render::Render;

#[derive(clap::Parser)]
/// Look up a bookmark or commit id
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
}

#[derive(Serialize)]
pub(crate) struct CommitResponsePair {
    pub id: String,
    pub lookup_output: LookupOutput,
}

#[derive(Serialize)]
pub(crate) struct LookupOutput {
    #[serde(skip)]
    pub requested: String,
    pub exists: bool,
    pub ids: BTreeMap<String, String>,
}

impl Render for LookupOutput {
    type Args = SchemeArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if self.exists {
            let schemes = args.scheme_string_set();
            render_commit_id(None, "\n", &self.requested, &self.ids, &schemes, w)?;
            write!(w, "\n")?;
        } else {
            bail!("{} does not exist\n", self.requested);
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

#[derive(Serialize)]
pub(crate) struct MultipleLookupOutput {
    pub responses: Vec<CommitResponsePair>,
}

impl Render for MultipleLookupOutput {
    type Args = SchemeArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        let schemes = args.scheme_string_set();
        let mut found_any = false;

        for response in &self.responses {
            if response.lookup_output.exists {
                write!(w, "{}: ", response.id)?;
                render_commit_id(
                    None,
                    " ",
                    &response.id,
                    &response.lookup_output.ids,
                    &schemes,
                    w,
                )?;
                write!(w, "\n")?;
                found_any = true;
            }
        }

        if !found_any {
            bail!("None of the commits were found");
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

async fn single_lookup(
    resolved_id: thrift::CommitId,
    requested_id: CommitId,
    repo: thrift::RepoSpecifier,
    identity_schemes: std::collections::BTreeSet<thrift::CommitIdentityScheme>,
    conn: ScsClient,
) -> Result<LookupOutput> {
    let commit = thrift::CommitSpecifier {
        repo,
        id: resolved_id,
        ..Default::default()
    };
    let params = thrift::CommitLookupParams {
        identity_schemes,
        ..Default::default()
    };
    let response = conn
        .commit_lookup(&commit, &params)
        .await
        .map_err(|e| e.handle_selection_error(&commit.repo))?;
    let ids = match &response.ids {
        Some(ids) => map_commit_ids(ids.values()),
        None => BTreeMap::new(),
    };
    Ok(LookupOutput {
        requested: requested_id.to_string(),
        exists: response.exists,
        ids,
    })
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_ids = args.commit_ids_args.clone().into_commit_ids();
    if commit_ids.is_empty() {
        bail!("expected at least one commit ID");
    }

    let conn = app.get_connection(Some(&repo.name))?;

    let resolved_commit_ids = resolve_commit_ids(&conn, &repo, &commit_ids).await?;
    let ids_index: HashMap<String, String> = resolved_commit_ids
        .iter()
        .map(|id| id.to_string())
        .zip(commit_ids.iter().map(|id| id.to_string()))
        .collect();

    if commit_ids.len() == 1 {
        let single_output = single_lookup(
            resolved_commit_ids[0].clone(),
            commit_ids[0].clone(),
            repo,
            args.scheme_args.clone().into_request_schemes(),
            conn,
        )
        .await?;
        app.target
            .render_one(&args.scheme_args, single_output)
            .await
    } else {
        let params = thrift::RepoMultipleCommitLookupParams {
            commit_ids: resolved_commit_ids,
            identity_schemes: args.scheme_args.clone().into_request_schemes(),
            ..Default::default()
        };
        let response = conn
            .repo_multiple_commit_lookup(&repo, &params)
            .await
            .map_err(|e| e.handle_selection_error(&repo))?;

        let mut commit_response_pairs = Vec::new();
        for aliases in response.responses.iter() {
            if let Some(requested_id) = ids_index.get(&aliases.commit_id.to_string()) {
                let ids = match &aliases.commit_lookup_response.ids {
                    Some(ids) => map_commit_ids(ids.values()),
                    None => BTreeMap::new(),
                };
                let lookup_output = LookupOutput {
                    requested: requested_id.to_string(),
                    exists: aliases.commit_lookup_response.exists,
                    ids,
                };
                commit_response_pairs.push(CommitResponsePair {
                    id: requested_id.to_string(),
                    lookup_output,
                });
            } else {
                bail!(
                    "Internal error: requested id {} is not in the response",
                    aliases.commit_id.to_string()
                );
            }
        }
        let output = MultipleLookupOutput {
            responses: commit_response_pairs,
        };

        app.target.render_one(&args.scheme_args, output).await
    }
}
