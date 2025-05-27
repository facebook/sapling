/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Diff two commits

use std::io::Write;

use anyhow::Result;
use anyhow::bail;
use clap::ValueEnum;
use commit_id_types::CommitIdsArgs;
use maplit::btreeset;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::SchemeArgs;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::library::diff::diff_files;
use crate::render::Render;

#[derive(ValueEnum, Clone, Copy, Debug)]
enum DiffFormat {
    RawDiff,
    MetadataDiff,
}

#[derive(clap::Parser)]
// Commit identity schemes to use as an intermediate step to resolve commit ids.
// (default: bonsai) - should be good for all cases. Overriding is only good for debugging or testing.
#[clap(mut_arg("schemes", |arg| arg.default_value("bonsai").hide(true)))]
/// Diff files between two commits
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
    #[clap(long, short)]
    /// Paths to diff.
    path: Option<Vec<String>>,
    #[clap(long)]
    /// Show copies/moves as adds/deletes.
    skip_copies_renames: bool,
    #[clap(long)]
    /// Only list differing paths instead of printing the diff.
    paths_only: bool,
    #[clap(long, conflicts_with = "paths_only")]
    /// Instead of generating a real diff let's generate a placeholder diff that just says that file differs.
    placeholders_only: bool,
    #[clap(long, short = 'O')]
    /// Generate diff in repository order.
    ordered: bool,
    #[clap(long, requires = "ordered")]
    /// Generate ordered diff after a given path.
    after: Option<String>,
    #[clap(long, short, default_value_t = 100, requires = "ordered")]
    /// Generate ordered diff for at most LIMIT files.
    limit: usize,
    #[clap(long, short = 's')]
    /// Limit the total size in bytes of returned diffs.
    diff_size_limit: Option<i64>,
    /// The format of the diff.
    #[clap(long, short = 'f', value_enum, default_value_t = DiffFormat::RawDiff)]
    diff_format: DiffFormat,
    /// Number of lines of unified context around differences.
    #[clap(long = "unified", short = 'U', default_value_t = 3)]
    context: i64,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
}

#[derive(Serialize)]
struct PathsOnlyOutput {
    files: Vec<thrift::CommitCompareFile>,
}

impl Render for PathsOnlyOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
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

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_ids = args.commit_ids_args.clone().into_commit_ids();
    if commit_ids.len() > 2 || commit_ids.is_empty() {
        bail!("expected 1 or 2 commit_ids (got {})", commit_ids.len())
    }
    let paths = args.path.clone();
    let conn = app.get_connection(Some(&repo.name))?;
    let commit_ids = resolve_commit_ids(&conn, &repo, &commit_ids).await?;
    let identity_schemes = args.scheme_args.clone().into_request_schemes();

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
        (commits.first(), commits[1].clone())
    };
    let ordered_params = if args.ordered {
        let after_path = args.after.clone();
        let limit: i64 = args.limit.try_into()?;
        Some(thrift::CommitCompareOrderedParams {
            after_path,
            limit,
            ..Default::default()
        })
    } else {
        None
    };
    let diff_format = match args.diff_format {
        DiffFormat::RawDiff => thrift::DiffFormat::RAW_DIFF,
        DiffFormat::MetadataDiff => thrift::DiffFormat::METADATA_DIFF,
    };

    let params = thrift::CommitCompareParams {
        other_commit_id: other_commit.map(|c| c.id.clone()),
        skip_copies_renames: args.skip_copies_renames,
        identity_schemes,
        paths,
        compare_items: btreeset! {thrift::CommitCompareItem::FILES},
        ordered_params,
        ..Default::default()
    };
    let response = conn
        .commit_compare(&base_commit, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;

    if args.paths_only {
        return app
            .target
            .render_one(
                &args,
                PathsOnlyOutput {
                    files: response.diff_files,
                },
            )
            .await;
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

    let placeholder_only = args.placeholders_only;
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
    app.target
        .render(
            &(),
            diff_files(
                &conn,
                base_commit.clone(),
                other_commit_id,
                paths_sizes,
                args.diff_size_limit,
                diff_format,
                args.context,
            ),
        )
        .await
}
