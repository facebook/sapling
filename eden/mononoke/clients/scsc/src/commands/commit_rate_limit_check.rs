/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use commit_id_types::CommitIdArgs;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::render::Render;

#[derive(clap::Parser)]
/// Check commit rate limits without pushing
///
/// Provide a commit and the bookmark you plan to push to.
/// The rate limit rules configured for the repository will be checked
/// against this commit and their outcomes will be reported.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(long)]
    /// Name of the bookmark you would push to if pushing for real
    to: String,
}

#[derive(Serialize)]
#[serde(tag = "status")]
enum RuleOutcome {
    Allowed,
    Exceeded {
        current_count: i64,
        max_commits: i64,
        window_secs: i64,
    },
}

#[derive(Serialize)]
struct RuleResult {
    rule_name: String,
    outcome: RuleOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    directories: Option<Vec<String>>,
}

#[derive(Serialize)]
struct RateLimitCheckOutput {
    commit: String,
    bookmark: String,
    passed: bool,
    rule_results: Vec<RuleResult>,
}

impl Render for RateLimitCheckOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        write!(
            w,
            "Rate limit check for {} to bookmark {}:\n\n",
            self.commit, self.bookmark
        )?;
        if self.rule_results.is_empty() {
            write!(w, "No rate limit rules configured.\n")?;
            return Ok(());
        }
        write!(
            w,
            "Outcome: {}\n\n",
            if self.passed { "PASSED" } else { "FAILED" }
        )?;
        for rule in &self.rule_results {
            write!(w, "{}", rule.rule_name)?;
            if let Some(user) = &rule.user_filter {
                write!(w, " (user: {user})")?;
            }
            if let Some(dirs) = &rule.directories {
                if !dirs.is_empty() {
                    write!(w, " (directories: [{}])", dirs.join(", "))?;
                }
            }
            write!(w, " => ")?;
            match &rule.outcome {
                RuleOutcome::Allowed => write!(w, "ALLOWED\n")?,
                RuleOutcome::Exceeded {
                    current_count,
                    max_commits,
                    window_secs,
                } => write!(
                    w,
                    "EXCEEDED: {current_count} commits in the last {window_secs} seconds (limit: {max_commits})\n"
                )?,
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
    let conn = app.get_connection(Some(&repo.name)).await?;
    let commit_id = resolve_commit_id(&conn, &repo, &original_commit_id).await?;
    let commit_specifier = thrift::CommitSpecifier {
        id: commit_id,
        repo,
        ..Default::default()
    };
    let bookmark = args.to.clone();

    let params = thrift::CommitRateLimitCheckParams {
        bookmark: bookmark.clone(),
        ..Default::default()
    };
    let response = conn
        .commit_rate_limit_check(&commit_specifier, &params)
        .await
        .map_err(|e| e.handle_selection_error(&commit_specifier.repo))?;

    let rule_results: Vec<RuleResult> = response
        .rule_results
        .into_iter()
        .map(|r| {
            let outcome = match r.outcome {
                thrift::CommitRateLimitRuleOutcome::allowed(_) => RuleOutcome::Allowed,
                thrift::CommitRateLimitRuleOutcome::exceeded(e) => RuleOutcome::Exceeded {
                    current_count: e.current_count,
                    max_commits: e.max_commits,
                    window_secs: e.window_secs,
                },
                thrift::CommitRateLimitRuleOutcome::UnknownField(id) => {
                    anyhow::bail!("Unknown rate limit outcome variant: {id}")
                }
            };
            Ok(RuleResult {
                rule_name: r.rule_name,
                outcome,
                user_filter: r.user_filter,
                directories: r.directories,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let output = RateLimitCheckOutput {
        commit: original_commit_id.to_string(),
        bookmark,
        passed: response.passed,
        rule_results,
    };
    app.target.render_one(&args, output).await
}
