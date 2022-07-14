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
use clap::App;
use clap::AppSettings;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use serde_derive::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::add_commit_id_args;
use crate::args::commit_id::add_scheme_args;
use crate::args::commit_id::get_commit_id;
use crate::args::commit_id::get_request_schemes;
use crate::args::commit_id::get_schemes;
use crate::args::commit_id::map_commit_id;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::connection::Connection;
use crate::lib::commit_id::render_commit_id;
use crate::render::Render;
use crate::render::RenderStream;

pub(super) const NAME: &str = "lookup-pushrebase-history";

#[allow(clippy::let_and_return)]
pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Find pushrebase history for a public commit by traversing mappings")
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_scheme_args(cmd);
    let cmd = add_commit_id_args(cmd);

    cmd
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
    fn render(&self, matches: &ArgMatches, w: &mut dyn Write) -> Result<()> {
        if self.exists {
            write!(w, "repo={}\n", self.repo_name)?;
            let schemes = get_schemes(matches);
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

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

#[derive(Serialize)]
struct PushrebaseLookupOutput {
    commit_lookups: Vec<CommitLookupOutput>,
}

impl Render for PushrebaseLookupOutput {
    fn render(&self, matches: &ArgMatches, w: &mut dyn Write) -> Result<()> {
        for (i, commit) in self.commit_lookups.iter().enumerate() {
            if i > 0 {
                write!(w, "--\n")?;
            }
            commit.render(matches, w)?;
        }
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(matches: &ArgMatches<'_>, connection: Connection) -> Result<RenderStream> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let pushrebase_history = connection
        .commit_lookup_pushrebase_history(
            &commit,
            &thrift::CommitLookupPushrebaseHistoryParams {
                ..Default::default()
            },
        )
        .await?;
    let lookup_params = thrift::CommitLookupParams {
        identity_schemes: get_request_schemes(matches),
        ..Default::default()
    };
    let commit_lookups: Vec<_> = stream::iter(pushrebase_history.history.clone())
        .map(|commit| connection.commit_lookup(&commit, &lookup_params))
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
    let output = Box::new(PushrebaseLookupOutput { commit_lookups });
    Ok(stream::once(async move { Ok(output as Box<dyn Render>) }).boxed())
}
