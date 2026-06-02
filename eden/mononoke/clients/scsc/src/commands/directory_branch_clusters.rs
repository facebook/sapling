/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Display directory branch clusters for a commit.

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

const DEFAULT_LIMIT: i64 = 1000;

#[derive(clap::Parser)]
/// List directory branch clusters for a commit
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(long, short, num_args = 1..)]
    /// Path or paths to filter clusters.  If provided, only clusters which are
    /// a path prefix of the given path are returned.
    path: Option<Vec<String>>,
    #[clap(long)]
    /// Resume listing after this path (for pagination).
    after_path: Option<String>,
    #[clap(long, default_value_t = DEFAULT_LIMIT)]
    /// Maximum number of clusters to return.
    limit: i64,
    #[clap(long)]
    /// Output in JSON format.
    json: bool,
}

#[derive(Serialize)]
struct DirectoryBranchClusterOutput {
    primary_path: String,
    secondary_paths: Vec<String>,
}

#[derive(Serialize)]
struct ClustersOutput {
    clusters: Vec<DirectoryBranchClusterOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_path: Option<String>,
}

impl Render for ClustersOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if self.clusters.is_empty() {
            writeln!(w, "No directory branch clusters found.")?;
        } else {
            for (i, cluster) in self.clusters.iter().enumerate() {
                if i > 0 {
                    writeln!(w)?;
                }
                writeln!(w, "Primary: {}", cluster.primary_path)?;
                if cluster.secondary_paths.is_empty() {
                    writeln!(w, "  Secondaries: (none)")?;
                } else {
                    writeln!(w, "  Secondaries:")?;
                    for secondary in &cluster.secondary_paths {
                        writeln!(w, "    - {secondary}")?;
                    }
                }
            }
            if let Some(last_path) = &self.last_path {
                writeln!(w, "(more results available, last_path: {last_path})")?;
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
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let conn = app.get_connection(Some(&repo.name)).await?;
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;

    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    };

    let paths = args.path.clone();

    let params = thrift::CommitDirectoryBranchClustersParams {
        paths,
        after_path: args.after_path.clone(),
        limit: args.limit,
        ..Default::default()
    };

    let response = conn
        .commit_directory_branch_clusters(&commit, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;

    let clusters: Vec<DirectoryBranchClusterOutput> = response
        .clusters
        .into_iter()
        .map(|cluster| DirectoryBranchClusterOutput {
            primary_path: cluster.primary_path,
            secondary_paths: cluster.secondary_paths,
        })
        .collect();

    let last_path = response.last_path;

    let output = ClustersOutput {
        clusters,
        last_path,
    };

    app.target.render_one(&args, output).await
}
