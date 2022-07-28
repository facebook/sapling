/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::Result;
use source_control::types as thrift;

use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitId;
use crate::args::commit_id::CommitIdArgs;
use crate::args::commit_id::SchemeArgs;
use crate::commands::lookup::LookupOutput;
use crate::connection::Connection;
use crate::ScscApp;

#[derive(clap::Parser)]
#[clap(group(
    clap::ArgGroup::new("hint")
    .args(&["hint-exact-commit", "hint-ancestor-of-commit", "hint-descendant-of-commit",
            "hint-ancestor-of-bookmark", "hint-descendant-of-bookmark"])
))]
/// Sync a commit between repositories
pub(super) struct CommandArgs {
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(long)]
    /// Source repository name
    source_repo: String,
    #[clap(long)]
    /// Target repository name
    target_repo: String,
    #[clap(long)]
    /// For Source Control use only. A commit to use as an Exact CandidateSelectionHint
    hint_exact_commit: Option<String>,
    #[clap(long)]
    /// For Source Control use only. A commit to use as an OnlyOrAncestorOfCommit CandidateSelectionHint
    hint_ancestor_of_commit: Option<String>,
    #[clap(long)]
    /// For Source Control use only. A commit to use as an OnlyOrDescendantOfCommit CandidateSelectionHint
    hint_descendant_of_commit: Option<String>,
    #[clap(long)]
    /// For Source Control use only. A bookmark to use as an OnlyOrAncestorOfBookmark CandidateSelectionHint
    hint_ancestor_of_bookmark: Option<String>,
    #[clap(long)]
    /// For Source Control use only. A bookmark to use as an OnlyOrDescendantOfBookmark CandidateSelectionHint
    hint_descendant_of_bookmark: Option<String>,
}

async fn build_commit_hint(
    connection: &Connection,
    target_repo: &thrift::RepoSpecifier,
    commit_id: &str,
    constructor: impl Fn(thrift::CommitId) -> thrift::CandidateSelectionHint,
) -> Result<thrift::CandidateSelectionHint> {
    let to_resolve = CommitId::Resolve(commit_id.to_string());
    let commit_id = resolve_commit_id(connection, target_repo, &to_resolve).await?;
    Ok(constructor(commit_id))
}

async fn build_hint(
    args: &CommandArgs,
    connection: &Connection,
    target_repo: &thrift::RepoSpecifier,
) -> Result<Option<thrift::CandidateSelectionHint>> {
    if let Some(commit_id) = &args.hint_exact_commit {
        Ok(Some(
            build_commit_hint(
                connection,
                target_repo,
                commit_id,
                thrift::CandidateSelectionHint::exact,
            )
            .await?,
        ))
    } else if let Some(commit_id) = &args.hint_ancestor_of_commit {
        Ok(Some(
            build_commit_hint(
                connection,
                target_repo,
                commit_id,
                thrift::CandidateSelectionHint::commit_ancestor,
            )
            .await?,
        ))
    } else if let Some(commit_id) = &args.hint_descendant_of_commit {
        Ok(Some(
            build_commit_hint(
                connection,
                target_repo,
                commit_id,
                thrift::CandidateSelectionHint::commit_descendant,
            )
            .await?,
        ))
    } else if let Some(bookmark) = args.hint_ancestor_of_bookmark.clone() {
        Ok(Some(thrift::CandidateSelectionHint::bookmark_ancestor(
            bookmark,
        )))
    } else if let Some(bookmark) = args.hint_descendant_of_bookmark.clone() {
        Ok(Some(thrift::CandidateSelectionHint::bookmark_descendant(
            bookmark,
        )))
    } else {
        Ok(None)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let source_repo = get_repo_specifier(args.source_repo.clone());
    let target_repo = get_repo_specifier(args.target_repo.clone());

    let commit_id = args.commit_id_args.clone().into_commit_id();
    let id = resolve_commit_id(&app.connection, &source_repo, &commit_id).await?;
    let hint = build_hint(&args, &app.connection, &target_repo).await?;

    let commit = thrift::CommitSpecifier {
        repo: source_repo,
        id,
        ..Default::default()
    };
    let params = thrift::CommitLookupXRepoParams {
        other_repo: target_repo,
        identity_schemes: args.scheme_args.clone().into_request_schemes(),
        candidate_selection_hint: hint,
        ..Default::default()
    };
    let response = app.connection.commit_lookup_xrepo(&commit, &params).await?;
    let ids = match &response.ids {
        Some(ids) => map_commit_ids(ids.values()),
        None => BTreeMap::new(),
    };

    let output = LookupOutput {
        requested: commit_id.to_string(),
        exists: response.exists,
        ids,
    };

    app.target.render_one(&args.scheme_args, output).await
}

fn get_repo_specifier(name: String) -> thrift::RepoSpecifier {
    thrift::RepoSpecifier {
        name,
        ..Default::default()
    }
}
