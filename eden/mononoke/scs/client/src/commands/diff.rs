/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Diff two commits

use anyhow::bail;
use anyhow::Result;
use maplit::btreeset;
use serde::Serialize;
use source_control as thrift;
use std::collections::BTreeSet;
use std::io::Write;

use crate::args::commit_id::resolve_commit_ids;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::repo::RepoArgs;
use crate::lib::diff::diff_files;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]
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
    #[clap(long, conflicts_with = "paths-only")]
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
    let commit_ids = resolve_commit_ids(&app.connection, &repo, &commit_ids).await?;
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
    let params = thrift::CommitCompareParams {
        other_commit_id: other_commit.map(|c| c.id.clone()),
        skip_copies_renames: args.skip_copies_renames,
        identity_schemes,
        paths,
        compare_items: btreeset! {thrift::CommitCompareItem::FILES},
        ordered_params,
        ..Default::default()
    };
    let response = app.connection.commit_compare(&base_commit, &params).await?;

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
                &app.connection,
                base_commit.clone(),
                other_commit_id,
                paths_sizes,
            ),
        )
        .await
}
