/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io::Write;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use chrono::naive::NaiveDateTime;
use chrono::DateTime;
use clap::App;
use clap::AppSettings;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use futures::future;
use futures::stream;
use futures_util::stream::StreamExt;
use source_control::types as thrift;

use crate::args::commit_id::add_multiple_commit_id_args;
use crate::args::commit_id::add_scheme_args;
use crate::args::commit_id::get_commit_ids;
use crate::args::commit_id::get_request_schemes;
use crate::args::commit_id::get_schemes;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::path::add_optional_path_args;
use crate::args::path::get_path;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::connection::Connection;
use crate::lib::commit::render_commit_info;
use crate::lib::commit::render_commit_summary;
use crate::lib::commit::CommitInfo as CommitInfoOutput;
use crate::render::Render;
use crate::render::RenderStream;

pub(super) const NAME: &str = "log";

const ARG_LIMIT: &str = "LIMIT";
const ARG_SKIP: &str = "SKIP";
const ARG_AFTER: &str = "AFTER";
const ARG_BEFORE: &str = "BEFORE";
const ARG_VERBOSE: &str = "VERBOSE";
const ARG_HISTORY_ACROSS_DELETIONS: &str = "HISTORY_ACROSS_DELETIONS";
const ARG_FOLOW_MUTABLE_HISTORY: &str = "FOLLOW_MUTABLE_HISTORY";

const ARG_LIMIT_DEFAULT: &str = "10";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Show the history of a commit or a path in a commit")
        .long_about(concat!(
            "Show the history of a commit or a path in a commit\n\n",
            "If a second commit id is provided, the results are limited to descendants ",
            "of that commit.",
        ))
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_scheme_args(cmd);
    let cmd = add_multiple_commit_id_args(cmd);
    let cmd = add_optional_path_args(cmd);
    let cmd = cmd
        .arg(
            Arg::with_name(ARG_LIMIT)
                .short("l")
                .long("limit")
                .takes_value(true)
                .default_value(ARG_LIMIT_DEFAULT)
                .help("Limit history length"),
        )
        .arg(
            Arg::with_name(ARG_SKIP)
                .short("s")
                .long("skip")
                .takes_value(true)
                .default_value("0")
                .help("Show history and skip first [SKIP] commits"),
        )
        .arg(
            Arg::with_name(ARG_AFTER)
                .long("after")
                .takes_value(true)
                .conflicts_with(ARG_SKIP)
                .help("Show only commits after the given date or timestamp. The given time must be after 1970-01-01 00:00:00 UTC.\nFormat: YYYY-MM-DD HH:MM:SS [+HH:MM]"),
        )
        .arg(
            Arg::with_name(ARG_BEFORE)
                .long("before")
                .takes_value(true)
                .conflicts_with(ARG_SKIP)
                .help("Show only commits before the given date or timestamp. The given time must be after 1970-01-01 00:00:00 UTC.\nFormat: YYYY-MM-DD HH:MM:SS [+HH:MM]"),
        )
        .arg(
            Arg::with_name(ARG_VERBOSE)
                .long("verbose")
                .short("v")
                .help("Show the full commit message of each commit"),
        )
        .arg(
            Arg::with_name(ARG_HISTORY_ACROSS_DELETIONS)
                .long("history-across-deletions")
                .help("Track history across deletion i.e. if a path was deleted then added back"),
        )
        .arg(
            Arg::with_name(ARG_FOLOW_MUTABLE_HISTORY)
                .long("follow-mutable-history")
                .help("Follow mutable overrides to the history that make it more user friendly and 'correct'"),
        );
    cmd
}

struct LogOutput {
    requested: String,
    schemes: HashSet<String>,
    history: Vec<CommitInfoOutput>,
}

impl Render for LogOutput {
    fn render(&self, matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        let verbose = matches.is_present(ARG_VERBOSE);
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

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, &self.history)?)
    }
}

fn convert_to_ts(matches: &ArgMatches, name: &str) -> Result<Option<i64>, Error> {
    if let Some(date_str) = matches.value_of(name) {
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
        return Err(format_err!(
            "The given date or timestamp must be after 1970-01-01 00:00:00 UTC: {:?}",
            date_str
        ));
    }

    Ok(None)
}

pub(super) async fn run(
    matches: &ArgMatches<'_>,
    connection: Connection,
) -> Result<RenderStream, Error> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_ids = get_commit_ids(matches)?;
    if commit_ids.len() > 2 || commit_ids.is_empty() {
        bail!("expected 1 or 2 commit_ids (got {})", commit_ids.len())
    }
    let ids = resolve_commit_ids(&connection, &repo, &commit_ids).await?;
    let id = ids[0].clone();
    let descendants_of = ids.get(1).cloned();
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let path = get_path(matches);

    let limit = matches
        .value_of(ARG_LIMIT)
        .expect("limit is required")
        .parse::<i32>()?;
    let skip = matches
        .value_of(ARG_SKIP)
        .expect("skip is required")
        .parse::<i32>()?;

    let before_timestamp = convert_to_ts(matches, ARG_BEFORE)?;
    let after_timestamp = convert_to_ts(matches, ARG_AFTER)?;
    let follow_history_across_deletions = matches.is_present(ARG_HISTORY_ACROSS_DELETIONS);
    let follow_mutable_file_history = Some(matches.is_present(ARG_FOLOW_MUTABLE_HISTORY));

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
                identity_schemes: get_request_schemes(matches),
                follow_history_across_deletions,
                descendants_of,
                exclude_changeset_and_ancestors: None,
                follow_mutable_file_history,
                ..Default::default()
            };
            connection
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
                identity_schemes: get_request_schemes(matches),
                descendants_of,
                exclude_changeset_and_ancestors: None,
                ..Default::default()
            };
            connection.commit_history(&commit, &params).await?.history
        }
    };

    let mut history = vec![];
    match response {
        thrift::History::commit_infos(commits) => {
            for commit in commits {
                let commit_info = CommitInfoOutput::try_from(&commit)?;
                history.push(commit_info);
            }

            let output: Box<dyn Render> = Box::new(LogOutput {
                history,
                requested: commit_ids[0].to_string(),
                schemes: get_schemes(matches),
            });
            Ok(stream::once(future::ok(output)).boxed())
        }
        thrift::History::UnknownField(id) => {
            Err(format_err!("Unknown thrift::History field id: {}", id))
        }
        _ => Err(format_err!("Unexpected thrift::History format")),
    }
}
