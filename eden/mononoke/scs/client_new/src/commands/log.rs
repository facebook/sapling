/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io::Write;

use anyhow::Result;
use chrono::naive::NaiveDateTime;
use chrono::DateTime;
use source_control::types as thrift;

use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::repo::RepoArgs;
use crate::lib::commit::render_commit_info;
use crate::lib::commit::render_commit_summary;
use crate::lib::commit::CommitInfo as CommitInfoOutput;
use crate::render::Render;
use crate::ScscApp;

/// Show the history of a commit or a path in a commit
///
/// If a second commit id is provided, the results are limited to descendants
/// of that commit.
#[derive(clap::Parser)]
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
    #[clap(long, short)]
    /// Optional path to query history
    path: Option<String>,
    #[clap(long, short, default_value_t = 10)]
    /// Limit history length
    limit: u32,
    #[clap(long, short)]
    /// Show history and skip first [SKIP] commits
    skip: Option<u32>,
    #[clap(long, conflicts_with = "skip")]
    /// Show only commits after the given date or timestamp. The given time must be after 1970-01-01 00:00:00 UTC.
    /// Format: YYYY-MM-DD HH:MM:SS [+HH:MM]
    after: Option<String>,
    #[clap(long, conflicts_with = "skip")]
    /// Show only commits before the given date or timestamp. The given time must be after 1970-01-01 00:00:00 UTC.
    /// Format: YYYY-MM-DD HH:MM:SS [+HH:MM]
    before: Option<String>,
    #[clap(long, short)]
    /// Show the full commit message of each commit
    verbose: bool,
    #[clap(long)]
    /// Track history across deletion i.e. if a path was deleted then added back
    history_across_deletions: bool,
    #[clap(long)]
    /// Follow mutable overrides to the history that make it more user friendly and 'correct'
    follow_mutable_history: bool,
}

struct LogOutput {
    requested: String,
    schemes: HashSet<String>,
    history: Vec<CommitInfoOutput>,
}

impl Render for LogOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        let verbose = args.verbose;
        for commit in &self.history {
            if verbose {
                render_commit_info(commit, &self.requested, &self.schemes, w)?;
                write!(w, "\n")?;
            } else {
                render_commit_summary(commit, &self.requested, &self.schemes, w)?;
                write!(w, "\n")?;
            }
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, &self.history)?)
    }
}

fn convert_to_ts(date_str: Option<&str>) -> Result<Option<i64>> {
    if let Some(date_str) = date_str {
        let ts = if let Ok(date) = DateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S %:z") {
            date.timestamp()
        } else if let Ok(naive) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
            naive.timestamp()
        } else {
            date_str.parse::<i64>()?
        };

        if ts > 0 {
            return Ok(Some(ts));
        }
        anyhow::bail!(
            "The given date or timestamp must be after 1970-01-01 00:00:00 UTC: {:?}",
            date_str
        )
    }

    Ok(None)
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_ids = args.commit_ids_args.clone().into_commit_ids();
    if commit_ids.len() > 2 || commit_ids.is_empty() {
        anyhow::bail!("expected 1 or 2 commit_ids (got {})", commit_ids.len())
    }
    let ids = resolve_commit_ids(&app.connection, &repo, &commit_ids).await?;
    let id = ids[0].clone();
    let descendants_of = ids.get(1).cloned();
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let path = args.path.clone();

    let limit: i32 = args.limit.try_into()?;
    let skip: i32 = args.skip.unwrap_or(0).try_into()?;

    let before_timestamp = convert_to_ts(args.before.as_deref())?;
    let after_timestamp = convert_to_ts(args.after.as_deref())?;
    let follow_history_across_deletions = args.history_across_deletions;
    let follow_mutable_file_history = Some(args.follow_mutable_history);
    let identity_schemes = args.scheme_args.clone().into_request_schemes();

    let response = match path {
        Some(path) => {
            let commit_and_path = thrift::CommitPathSpecifier {
                commit,
                path,
                ..Default::default()
            };
            let params = thrift::CommitPathHistoryParams {
                format: thrift::HistoryFormat::COMMIT_INFO,
                limit,
                skip,
                before_timestamp,
                after_timestamp,
                identity_schemes,
                follow_history_across_deletions,
                descendants_of,
                exclude_changeset_and_ancestors: None,
                follow_mutable_file_history,
                ..Default::default()
            };
            app.connection
                .commit_path_history(&commit_and_path, &params)
                .await?
                .history
        }
        None => {
            let params = thrift::CommitHistoryParams {
                format: thrift::HistoryFormat::COMMIT_INFO,
                limit,
                skip,
                before_timestamp,
                after_timestamp,
                identity_schemes,
                descendants_of,
                exclude_changeset_and_ancestors: None,
                ..Default::default()
            };
            app.connection
                .commit_history(&commit, &params)
                .await?
                .history
        }
    };

    let mut history = vec![];
    match response {
        thrift::History::commit_infos(commits) => {
            for commit in commits {
                let commit_info = CommitInfoOutput::try_from(&commit)?;
                history.push(commit_info);
            }

            let output = LogOutput {
                history,
                requested: commit_ids[0].to_string(),
                schemes: args.scheme_args.scheme_string_set(),
            };
            app.target.render_one(&args, output).await
        }
        thrift::History::UnknownField(id) => {
            anyhow::bail!("Unknown thrift::History field id: {}", id)
        }
        _ => anyhow::bail!("Unexpected thrift::History format"),
    }
}
