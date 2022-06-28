/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Diff two commits

use anyhow::bail;
use anyhow::Error;
use clap::App;
use clap::AppSettings;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream;
use futures_util::stream::StreamExt;
use maplit::btreeset;
use source_control as thrift;
use std::collections::BTreeSet;
use std::io::Write;

use crate::args::commit_id::add_multiple_commit_id_args;
use crate::args::commit_id::get_commit_ids;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::path::add_optional_multiple_path_args;
use crate::args::path::get_paths;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::connection::Connection;
use crate::lib::diff::diff_files;
use crate::render::Render;
use crate::render::RenderStream;
use serde_derive::Serialize;

pub(super) const NAME: &str = "diff";

const ARG_SKIP_COPY_INFO: &str = "skip-copies-renames";
const ARG_PATHS_ONLY: &str = "paths-only";
const ARG_PLACEHOLDERS_ONLY: &str = "placeholders-only";
const ARG_ORDERED: &str = "ORDERED";
const ARG_AFTER: &str = "AFTER";
const ARG_LIMIT: &str = "LIMIT";

const ARG_LIMIT_DEFAULT: &str = "100";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Diff files between two commits")
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_multiple_commit_id_args(cmd);
    let cmd = add_optional_multiple_path_args(cmd);
    cmd.arg(
        Arg::with_name(ARG_SKIP_COPY_INFO)
            .long(ARG_SKIP_COPY_INFO)
            .help("Show copies/moves as adds/deletes."),
    )
    .arg(
        Arg::with_name(ARG_PATHS_ONLY)
            .long(ARG_PATHS_ONLY)
            .help("Only list differing paths instead of printing the diff."),
    )
    .arg(
        Arg::with_name(ARG_PLACEHOLDERS_ONLY)
            .long(ARG_PLACEHOLDERS_ONLY)
            .conflicts_with(ARG_PATHS_ONLY)
            .help("Instead of generating a real diff let's generate a placeholder diff that just says that file differs"),
    )
    .arg(
        Arg::with_name(ARG_ORDERED)
            .long("ordered")
            .short("O")
            .help("Generate diff in repository order")
        )
    .arg(
        Arg::with_name(ARG_AFTER)
            .long("after")
            .takes_value(true)
            .help("Generate ordered diff after a given path")
        )
    .arg(
        Arg::with_name(ARG_LIMIT)
            .long("limit")
            .short("l")
            .default_value(ARG_LIMIT_DEFAULT)
            .help("Generate ordered diff for at most LIMIT files")
        )
}

#[derive(Serialize)]
struct PathsOnlyOutput {
    files: Vec<thrift::CommitCompareFile>,
}

impl Render for PathsOnlyOutput {
    fn render(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        for file in &self.files {
            match (file.base_file.as_ref(), file.other_file.as_ref()) {
                // The letters as in git-status
                (Some(base_file), None) => writeln!(w, "A {}", base_file.path)?,
                (None, Some(other_file)) => writeln!(w, "D {}", other_file.path)?,
                (Some(base_file), Some(other_file)) => match file.copy_info {
                    thrift::CopyInfo::NONE => writeln!(w, "M {}", base_file.path)?,
                    thrift::CopyInfo::MOVE => {
                        writeln!(w, "R {} -> {}", other_file.path, base_file.path)?;
                    }
                    thrift::CopyInfo::COPY => {
                        writeln!(w, "C {} -> {}", other_file.path, base_file.path)?;
                    }
                    // There is no more possibilities but thrift doesn't know about it.
                    _ => bail!("unrecognized CopyInfo!"),
                },
                // Also rather impossible
                (None, None) => bail!("empty file pair received!"),
            }
        }
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(
    matches: &ArgMatches<'_>,
    connection: Connection,
) -> Result<RenderStream, Error> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_ids = get_commit_ids(matches)?;
    if commit_ids.len() > 2 || commit_ids.len() < 1 {
        bail!("expected 1 or 2 commit_ids (got {})", commit_ids.len())
    }
    let paths = get_paths(matches);
    let commit_ids = resolve_commit_ids(&connection, &repo, &commit_ids).await?;
    let mut identity_schemes = BTreeSet::new();
    identity_schemes.insert(thrift::CommitIdentityScheme::BONSAI);

    let commits: Vec<_> = commit_ids
        .into_iter()
        .map(|id| thrift::CommitSpecifier {
            repo: repo.clone(),
            id,
            ..Default::default()
        })
        .collect();
    let (other_commit, base_commit) = if commits.len() == 1 {
        (None, commits[0].clone())
    } else {
        (commits.get(0), commits[1].clone())
    };
    let ordered_params = if matches.is_present(ARG_ORDERED) {
        let after_path = matches.value_of(ARG_AFTER).map(ToString::to_string);
        let limit = matches
            .value_of(ARG_LIMIT)
            .unwrap_or(ARG_LIMIT_DEFAULT)
            .parse::<i64>()?;
        Some(thrift::CommitCompareOrderedParams {
            after_path,
            limit,
            ..Default::default()
        })
    } else {
        if matches.is_present(ARG_AFTER) {
            bail!("--after requires --ordered");
        }
        // Check occurrences as limit has a default.
        if matches.occurrences_of(ARG_LIMIT) > 0 {
            bail!("--limit requires --ordered");
        }
        None
    };
    let params = thrift::CommitCompareParams {
        other_commit_id: other_commit.map(|c| c.id.clone()),
        skip_copies_renames: matches.is_present(ARG_SKIP_COPY_INFO),
        identity_schemes,
        paths,
        compare_items: btreeset! {thrift::CommitCompareItem::FILES},
        ordered_params,
        ..Default::default()
    };
    let response = connection.commit_compare(&base_commit, &params).await?;

    if matches.is_present(ARG_PATHS_ONLY) {
        return Ok(stream::once(async move {
            Ok(Box::new(PathsOnlyOutput {
                files: response.diff_files,
            }) as Box<dyn Render>)
        })
        .boxed());
    }

    let other_commit_id = match response.other_commit_ids {
        None => None,
        Some(other_commit_ids) => {
            let (_scheme, other_commit_id) = other_commit_ids
                .into_iter()
                .next()
                .expect("expected commit id");
            Some(other_commit_id)
        }
    };

    let placeholder_only = matches.is_present(ARG_PLACEHOLDERS_ONLY);
    let paths_sizes = response.diff_files.iter().map(|diff_file| {
        let pair_size = diff_file.base_file.as_ref().map_or(0, |f| f.info.file_size)
            + diff_file
                .other_file
                .as_ref()
                .map_or(0, |f| f.info.file_size);
        (
            thrift::CommitFileDiffsParamsPathPair {
                base_path: diff_file.base_file.as_ref().map(|f| f.path.clone()),
                other_path: diff_file.other_file.as_ref().map(|f| f.path.clone()),
                copy_info: diff_file.copy_info,
                generate_placeholder_diff: Some(
                    placeholder_only || pair_size > thrift::COMMIT_FILE_DIFFS_SIZE_LIMIT,
                ),
                ..Default::default()
            },
            pair_size,
        )
    });
    diff_files(
        &connection,
        base_commit.clone(),
        other_commit_id,
        paths_sizes,
    )
}
