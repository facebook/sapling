/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Error;
use anyhow::Result;
use clap::App;
use clap::AppSettings;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream::StreamExt;
use futures::stream::{self};
use serde_derive::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::add_commit_id_args;
use crate::args::commit_id::get_commit_id;
use crate::args::commit_id::resolve_commit_id;
use crate::args::pushvars::add_pushvar_args;
use crate::args::pushvars::get_pushvars;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::connection::Connection;
use crate::render::Render;
use crate::render::RenderStream;

pub(super) const NAME: &str = "run-hooks";

const ARG_NAME: &str = "BOOKMARK_NAME";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Run hooks on a commit without pushing it")
        .long_about(concat!(
            "Run hooks on a commit\n\n",
            "Provide a commit and the bookmark you plan to push to. ",
            "The hooks that would run when you push this commit to bookmark will run now ",
            "and their outcomes will be reported. A success does NOT guarantee ",
            "the commit will successfully land (e.g. conflicts may prevent landing)."
        ))
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_commit_id_args(cmd);
    let cmd = add_pushvar_args(cmd);
    cmd.arg(
        Arg::with_name(ARG_NAME)
            .long("to")
            .takes_value(true)
            .help("Name of the bookmark you would push to if pushing for real")
            .required(true),
    )
}

#[derive(Serialize)]
#[serde(tag = "status")]
enum HookOutcome {
    Accepted,
    Rejected { reason: String },
}

#[derive(Serialize)]
struct RunHooksOutput {
    commit: String,
    bookmark: String,
    outcomes: BTreeMap<String, HookOutcome>,
}

impl Render for RunHooksOutput {
    fn render(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        write!(
            w,
            "Hook outcomes when dry-run landing {} to bookmark {}:\n\n",
            self.commit, self.bookmark
        )?;
        for (hook_name, outcome) in &self.outcomes {
            write!(w, "{} => ", hook_name)?;
            match outcome {
                HookOutcome::Accepted => write!(w, "ACCEPTED\n")?,
                HookOutcome::Rejected { reason } => write!(w, "REJECTED: {}\n", reason)?,
            };
        }
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(matches: &ArgMatches<'_>, connection: Connection) -> Result<RenderStream> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let original_commit_id = get_commit_id(matches)?;
    let commit_id = resolve_commit_id(&connection, &repo, &original_commit_id).await?;
    let commit_specifier = thrift::CommitSpecifier {
        id: commit_id,
        repo,
        ..Default::default()
    };
    let bookmark: String = matches.value_of(ARG_NAME).expect("name is required").into();
    let pushvars = get_pushvars(matches)?;

    let params = thrift::CommitRunHooksParams {
        bookmark: bookmark.clone(),
        pushvars,
        ..Default::default()
    };
    let response = connection
        .commit_run_hooks(&commit_specifier, &params)
        .await?;
    let outcomes = response
        .outcomes
        .into_iter()
        .map(|(name, outcome)| {
            Ok((
                name,
                match outcome {
                    thrift::HookOutcome::accepted(_) => HookOutcome::Accepted,
                    thrift::HookOutcome::rejected(rej) => HookOutcome::Rejected {
                        reason: rej.long_description,
                    },
                    thrift::HookOutcome::UnknownField(_) => anyhow::bail!("Unknown hook outcome"),
                },
            ))
        })
        .collect::<Result<_>>()?;
    let output = Box::new(RunHooksOutput {
        commit: original_commit_id.to_string(),
        bookmark,
        outcomes,
    });
    Ok(stream::once(async move { Ok(output as Box<dyn Render>) }).boxed())
}
