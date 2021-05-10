/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Diff two commits

use anyhow::{bail, Error};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use futures::stream;
use futures_util::stream::StreamExt;
use maplit::btreeset;
use source_control as thrift;
use std::collections::BTreeSet;
use std::io::Write;

use crate::args::commit_id::{add_multiple_commit_id_args, get_commit_ids, resolve_commit_ids};
use crate::args::path::{add_optional_multiple_path_args, get_paths};
use crate::args::repo::{add_repo_args, get_repo_specifier};
use crate::connection::Connection;
use crate::lib::diff::diff_files;
use crate::render::{Render, RenderStream};
use serde_derive::Serialize;

pub(super) const NAME: &str = "diff";

const ARG_SKIP_COPY_INFO: &str = "skip-copies-renames";
const ARG_PATHS_ONLY: &str = "paths-only";
const ARG_PLACEHOLDERS_ONLY: &str = "placeholders-only";

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
        })
        .collect();
    let (other_commit, base_commit) = if commits.len() == 1 {
        (None, commits[0].clone())
    } else {
        (commits.get(0), commits[1].clone())
    };
    let params = thrift::CommitCompareParams {
        other_commit_id: other_commit.map(|c| c.id.clone()),
        skip_copies_renames: matches.is_present(ARG_SKIP_COPY_INFO),
        identity_schemes,
        paths,
        compare_items: btreeset! {thrift::CommitCompareItem::FILES},
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

    let (_scheme, other_commit_id) = response
        .other_commit_ids
        .into_iter()
        .next()
        .expect("expected commit id");

    let placeholder_only = matches.is_present(ARG_PLACEHOLDERS_ONLY);
    let paths_sizes = response.diff_files.iter().map(|diff_file| {
        (
            thrift::CommitFileDiffsParamsPathPair {
                base_path: diff_file.base_file.as_ref().map(|f| f.path.clone()),
                other_path: diff_file.other_file.as_ref().map(|f| f.path.clone()),
                copy_info: diff_file.copy_info,
                generate_placeholder_diff: Some(placeholder_only),
            },
            (diff_file
                .base_file
                .as_ref()
                .map(|f| f.info.file_size)
                .unwrap_or(0)
                + diff_file
                    .other_file
                    .as_ref()
                    .map(|f| f.info.file_size)
                    .unwrap_or(0)),
        )
    });
    diff_files(
        &connection,
        base_commit.clone(),
        other_commit_id,
        paths_sizes,
    )
}
