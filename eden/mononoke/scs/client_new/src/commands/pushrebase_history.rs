/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::bail;
use anyhow::Result;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use serde::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::map_commit_id;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::repo::RepoArgs;
use crate::lib::commit_id::render_commit_id;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Find pushrebase history for a public commit by traversing mappings
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
}

#[derive(Serialize)]
struct CommitLookupOutput {
    repo_name: String,
    #[serde(skip)]
    requested: String,
    exists: bool,
    ids: BTreeMap<String, String>,
}

impl Render for CommitLookupOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if self.exists {
            write!(w, "repo={}\n", self.repo_name)?;
            let schemes = args.scheme_args.scheme_string_set();
            render_commit_id(None, "\n", &self.requested, &self.ids, &schemes, w)?;
            write!(w, "\n")?;
        } else {
            bail!(
                "{} does not exist in repo {}\n",
                self.requested,
                self.repo_name
            );
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

#[derive(Serialize)]
struct PushrebaseLookupOutput {
    commit_lookups: Vec<CommitLookupOutput>,
}

impl Render for PushrebaseLookupOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        for (i, commit) in self.commit_lookups.iter().enumerate() {
            if i > 0 {
                write!(w, "--\n")?;
            }
            commit.render(args, w)?;
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
    let id = resolve_commit_id(&app.connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let pushrebase_history = app
        .connection
        .commit_lookup_pushrebase_history(
            &commit,
            &thrift::CommitLookupPushrebaseHistoryParams {
                ..Default::default()
            },
        )
        .await?;
    let lookup_params = thrift::CommitLookupParams {
        identity_schemes: args.scheme_args.clone().into_request_schemes(),
        ..Default::default()
    };
    let commit_lookups: Vec<_> = stream::iter(pushrebase_history.history.clone())
        .map(|commit| app.connection.commit_lookup(&commit, &lookup_params))
        .buffered(10)
        .try_collect()
        .await?;
    let commit_lookups: Vec<_> = pushrebase_history
        .history
        .into_iter()
        .zip(commit_lookups)
        .filter_map(|(commit, commit_lookup)| {
            let ids = match &commit_lookup.ids {
                Some(ids) => map_commit_ids(ids.values()),
                None => BTreeMap::new(),
            };

            if let Some((_, id)) = map_commit_id(&commit.id) {
                Some(CommitLookupOutput {
                    repo_name: commit.repo.name,
                    requested: id,
                    exists: commit_lookup.exists,
                    ids,
                })
            } else {
                None
            }
        })
        .collect();
    let output = PushrebaseLookupOutput { commit_lookups };
    app.target.render_one(&args, output).await
}
