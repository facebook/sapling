/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io::Write;

use anyhow::Result;
use commit_id_types::CommitIdNames;
use commit_id_types::NamedCommitIdsArgs;
use scs_client_raw::thrift;

use crate::ScscApp;
use crate::args::commit_id::SchemeArgs;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::resolve_optional_commit_id;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::library::commit::CommitInfo as CommitInfoOutput;
use crate::library::commit::render_commit_info;
use crate::library::commit::render_commit_summary;
use crate::render::Render;
use crate::util::convert_to_ts;

#[derive(Copy, Clone)]
struct LogCommitIdNames;

impl CommitIdNames for LogCommitIdNames {
    const NAMES: &'static [(&'static str, &'static str)] = &[
        (
            "descendants-of",
            "Include only descendants of the next commit",
        ),
        (
            "exclude-ancestors-of",
            "Exclude ancestors of the next commit",
        ),
    ];
}

/// Show the history of a commit or a path in a commit
///
/// If you want to restrict the returned commits to the descendants of another
/// commit (inclusive), then specify the other commit using '--descendants-of'.
///
/// scsc log -i HEAD --descendants-of -i BASE
///
/// If you want to exclude ancestors of another commit (inclusive), then specify
/// the other commit using '--exclude-ancestors-of'.
///
/// scsc log -i HEAD --exclude-ancestors-of -i BRANCH
#[derive(clap::Parser)]
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_ids_args: NamedCommitIdsArgs<LogCommitIdNames>,
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
    /// Show only the linear history of the commit, ignoring merge commits.
    #[clap(
        long,
        conflicts_with = "after",
        conflicts_with = "before",
        conflicts_with = "path"
    )]
    linear: bool,
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

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_ids = args.commit_ids_args.positional_commit_ids();
    if commit_ids.len() > 2 || commit_ids.is_empty() {
        anyhow::bail!("expected 1 or 2 commit_ids (got {})", commit_ids.len())
    }
    let conn = app.get_connection(Some(&repo.name))?;
    let ids = resolve_commit_ids(&conn, &repo, commit_ids).await?;
    let id = ids[0].clone();
    let positional_descendants_of = ids.get(1).cloned();
    let named_descendants_of = resolve_optional_commit_id(
        &conn,
        &repo,
        args.commit_ids_args
            .named_commit_ids()
            .get("descendants-of"),
    )
    .await?;
    if positional_descendants_of.is_some() && named_descendants_of.is_some() {
        anyhow::bail!("descendants-of must be specified either positionally or by name, not both");
    }
    let descendants_of = positional_descendants_of.xor(named_descendants_of);
    let exclude_changeset_and_ancestors = resolve_optional_commit_id(
        &conn,
        &repo,
        args.commit_ids_args
            .named_commit_ids()
            .get("exclude-ancestors-of"),
    )
    .await?;
    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
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
                exclude_changeset_and_ancestors,
                follow_mutable_file_history,
                ..Default::default()
            };
            conn.commit_path_history(&commit_and_path, &params)
                .await
                .map_err(|e| e.handle_selection_error(&repo))?
                .history
        }
        None => {
            if args.linear {
                let params = thrift::CommitLinearHistoryParams {
                    format: thrift::HistoryFormat::COMMIT_INFO,
                    limit,
                    skip: skip as i64,
                    identity_schemes,
                    descendants_of,
                    exclude_changeset_and_ancestors,
                    ..Default::default()
                };
                conn.commit_linear_history(&commit, &params)
                    .await
                    .map_err(|e| e.handle_selection_error(&repo))?
                    .history
            } else {
                let params = thrift::CommitHistoryParams {
                    format: thrift::HistoryFormat::COMMIT_INFO,
                    limit,
                    skip,
                    before_timestamp,
                    after_timestamp,
                    identity_schemes,
                    descendants_of,
                    exclude_changeset_and_ancestors,
                    ..Default::default()
                };
                conn.commit_history(&commit, &params)
                    .await
                    .map_err(|e| e.handle_selection_error(&repo))?
                    .history
            }
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
