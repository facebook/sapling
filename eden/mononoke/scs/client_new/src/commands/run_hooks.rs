/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use serde::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::pushvars::PushvarArgs;
use crate::args::repo::RepoArgs;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Run hooks on a commit without pushing it
///
/// Provide a commit and the bookmark you plan to push to.
/// The hooks that would run when you push this commit to bookmark will run now
/// and their outcomes will be reported. A success does NOT guarantee
/// the commit will successfully land (e.g. conflicts may prevent landing).
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(flatten)]
    pushvar_args: PushvarArgs,
    #[clap(long)]
    /// Name of the bookmark you would push to if pushing for real
    to: String,
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
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
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

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let original_commit_id = args.commit_id_args.clone().into_commit_id();
    let commit_id = resolve_commit_id(&app.connection, &repo, &original_commit_id).await?;
    let commit_specifier = thrift::CommitSpecifier {
        id: commit_id,
        repo,
        ..Default::default()
    };
    let bookmark: String = args.to.clone();
    let pushvars = args.pushvar_args.clone().into_pushvars();

    let params = thrift::CommitRunHooksParams {
        bookmark: bookmark.clone(),
        pushvars,
        ..Default::default()
    };
    let response = app
        .connection
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
    let output = RunHooksOutput {
        commit: original_commit_id.to_string(),
        bookmark,
        outcomes,
    };
    app.target.render_one(&args, output).await
}
