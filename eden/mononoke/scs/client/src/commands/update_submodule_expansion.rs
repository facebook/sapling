/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::repo::RepoArgs;
use crate::library::commit_id::render_commit_id;
use crate::render::Render;
use crate::util::convert_to_ts;
use crate::ScscApp;

/// Update a submodule expansion
#[derive(clap::Parser)]
pub(super) struct CommandArgs {
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    /// Large repo commit that will be the base commit for the generated commit
    /// updating the submodule expansion.
    #[clap(
        flatten,
        help = "Commit where the submodule expansion should be updated"
    )]
    base_commit_id: CommitIdArgs,
    #[clap(flatten, help = "Repo where the submodule expansion will be updated")]
    /// Large repo containing the expansion being updated
    large_repo_args: RepoArgs,

    #[clap(long, short = 'p')]
    submodule_expansion_path: String,
    /// New submodule git commit to expand.
    /// If none is provided, the user has to explicitly pass`--delete` to delete
    /// the submodule expansion.
    #[clap(
        long,
        short = 'n',
        conflicts_with = "delete",
        help = "New submodule git commit to point to"
    )]
    new_submodule_git_commit: Option<String>,
    // If a new submodule commit is not provided, the user is required to
    // explicitly pass the `--delete` flag, so they are aware that the submodule
    // will be deleted.
    #[clap(
        long,
        required_unless_present = "new_submodule_git_commit",
        help = "Delete submodule"
    )]
    delete: bool,
    /// The author date for the commit updating the submodule expansion.
    /// Format: YYYY-MM-DD HH:MM:SS [+HH:MM]
    #[clap(
        long,
        short = 'd',
        help = "Date to use in the generated commit. Defaults to now."
    )]
    author_date: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SubmoduleExpansionUpdateOutput {
    #[serde(skip)]
    pub requested: String,
    pub ids: BTreeMap<String, String>,
}

impl Render for SubmoduleExpansionUpdateOutput {
    type Args = SchemeArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        let schemes = args.scheme_string_set();
        render_commit_id(None, "\n", &self.requested, &self.ids, &schemes, w)?;
        write!(w, "\n")?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let large_repo = args.large_repo_args.clone().into_repo_specifier();
    let large_repo_conn = app.get_connection(Some(&large_repo.name))?;

    let new_submodule_commit_or_delete = args.new_submodule_git_commit.map(|commit_hash| {
        // TODO(T179531912): support other hashes
        thrift::CommitId::git(commit_hash.into_bytes())
    });
    let base_commit_id_arg = args.base_commit_id.clone().into_commit_id();
    let base_commit_id = resolve_commit_id(&large_repo_conn, &large_repo, &base_commit_id_arg)
        .await
        .context("Resolving commit id")?;

    let mb_author_date_ts = convert_to_ts(args.author_date.as_deref())?;
    let mb_author_date = mb_author_date_ts.map(|ts| thrift::DateTime {
        timestamp: ts,
        ..Default::default()
    });
    // TODO(T179531912): support message and author
    let commit_info = thrift::RepoUpdateSubmoduleExpansionCommitInfo {
        author_date: mb_author_date,
        ..Default::default()
    };

    let params = thrift::RepoUpdateSubmoduleExpansionParams {
        large_repo,
        base_commit_id,
        submodule_expansion_path: args.submodule_expansion_path,
        new_submodule_commit_or_delete,
        identity_schemes: args.scheme_args.clone().into_request_schemes(),
        commit_info: Some(commit_info),
        ..Default::default()
    };
    let response = large_repo_conn
        .repo_update_submodule_expansion(&params)
        .await?;

    let output = SubmoduleExpansionUpdateOutput {
        requested: "Commit".to_string(),
        ids: map_commit_ids(response.ids.values()),
    };

    app.target.render_one(&args.scheme_args, output).await
}
