/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::format_err;
use anyhow::Error;
use clap::App;
use clap::AppSettings;
use clap::Arg;
use clap::ArgGroup;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream;
use futures_util::stream::StreamExt;
use source_control::types as thrift;

use crate::args::commit_id::add_commit_id_args;
use crate::args::commit_id::add_scheme_args;
use crate::args::commit_id::get_commit_id;
use crate::args::commit_id::get_request_schemes;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitId;
use crate::commands::lookup::LookupOutput;
use crate::connection::Connection;
use crate::render::Render;
use crate::render::RenderStream;

pub(super) const NAME: &str = "xrepo-lookup";

const ARG_SOURCE_REPO: &str = "SOURCE_REPO";
const ARG_TARGET_REPO: &str = "TARGET_REPO";
const ARG_EXACT_HINT: &str = "HINT_EXACT_COMMIT";
const ARG_ANCESTOR_OF_COMMIT_HINT: &str = "HINT_ANCESTOR_OF_COMMIT";
const ARG_ANCESTOR_OF_BOOKMARK_HINT: &str = "HINT_ANCESTOR_OF_BOOKMARK";
const ARG_DESCENDANT_OF_COMMIT_HINT: &str = "HINT_DESCENDANT_OF_COMMIT";
const ARG_DESCENDANT_OF_BOOKMARK_HINT: &str = "HINT_DESCENDANT_OF_BOOKMARK";
const ARG_GROUP_HINT: &str = "CANDIDATE_SELECTION_HINT";

fn add_hint_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_EXACT_HINT)
            .long("hint-exact-commit")
            .help("For Source Control use only. A commit to use as an Exact CandidateSelectionHint")
            .takes_value(true)
            .required(false),
    )
    .arg(
        Arg::with_name(ARG_ANCESTOR_OF_COMMIT_HINT)
            .long("hint-ancestor-of-commit")
            .help("For Source Control use only. A commit to use as an OnlyOrAncestorOfCommit CandidateSelectionHint")
            .takes_value(true)
            .required(false),
    )
    .arg(
        Arg::with_name(ARG_DESCENDANT_OF_COMMIT_HINT)
            .long("hint-descendant-of-commit")
            .help("For Source Control use only. A commit to use as an OnlyOrDescendantOfCommit CandidateSelectionHint")
            .takes_value(true)
            .required(false),
    )
    .arg(
        Arg::with_name(ARG_ANCESTOR_OF_BOOKMARK_HINT)
            .long("hint-ancestor-of-bookmark")
            .help("For Source Control use only. A bookmark to use as an OnlyOrAncestorOfBookmark CandidateSelectionHint")
            .takes_value(true)
            .required(false),
    )
    .arg(
        Arg::with_name(ARG_DESCENDANT_OF_BOOKMARK_HINT)
            .long("hint-descendant-of-bookmark")
            .help("For Source Control use only. A bookmark to use as an OnlyOrDescendantOfBookmark CandidateSelectionHint")
            .takes_value(true)
            .required(false),
    )
    .group(ArgGroup::with_name(ARG_GROUP_HINT).args(&[
        ARG_EXACT_HINT,
        ARG_ANCESTOR_OF_COMMIT_HINT,
        ARG_ANCESTOR_OF_BOOKMARK_HINT,
        ARG_DESCENDANT_OF_COMMIT_HINT,
        ARG_DESCENDANT_OF_BOOKMARK_HINT,
    ]))
}

#[allow(clippy::let_and_return)]
pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Sync a commit between repositories")
        .setting(AppSettings::ColoredHelp)
        .arg(
            Arg::with_name(ARG_SOURCE_REPO)
                .long("source-repo")
                .help("Source repository name")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(ARG_TARGET_REPO)
                .long("target-repo")
                .help("Target repository name")
                .takes_value(true)
                .required(true),
        );
    let cmd = add_hint_args(cmd);
    let cmd = add_scheme_args(cmd);
    let cmd = add_commit_id_args(cmd);

    cmd
}

async fn build_commit_hint(
    connection: &Connection,
    target_repo: &thrift::RepoSpecifier,
    commit_id: &str,
    constructor: impl Fn(thrift::CommitId) -> thrift::CandidateSelectionHint,
) -> Result<thrift::CandidateSelectionHint, Error> {
    let to_resolve = CommitId::Resolve(commit_id.to_string());
    let commit_id = resolve_commit_id(connection, target_repo, &to_resolve).await?;
    Ok(constructor(commit_id))
}

async fn build_hint(
    matches: &ArgMatches<'_>,
    connection: &Connection,
    target_repo: &thrift::RepoSpecifier,
) -> Result<Option<thrift::CandidateSelectionHint>, Error> {
    if let Some(commit_id) = matches.value_of(ARG_EXACT_HINT) {
        Ok(Some(
            build_commit_hint(
                connection,
                target_repo,
                commit_id,
                thrift::CandidateSelectionHint::exact,
            )
            .await?,
        ))
    } else if let Some(commit_id) = matches.value_of(ARG_ANCESTOR_OF_COMMIT_HINT) {
        Ok(Some(
            build_commit_hint(
                connection,
                target_repo,
                commit_id,
                thrift::CandidateSelectionHint::commit_ancestor,
            )
            .await?,
        ))
    } else if let Some(commit_id) = matches.value_of(ARG_DESCENDANT_OF_COMMIT_HINT) {
        Ok(Some(
            build_commit_hint(
                connection,
                target_repo,
                commit_id,
                thrift::CandidateSelectionHint::commit_descendant,
            )
            .await?,
        ))
    } else if let Some(bookmark) = matches.value_of(ARG_ANCESTOR_OF_BOOKMARK_HINT) {
        Ok(Some(thrift::CandidateSelectionHint::bookmark_ancestor(
            bookmark.to_string(),
        )))
    } else if let Some(bookmark) = matches.value_of(ARG_DESCENDANT_OF_BOOKMARK_HINT) {
        Ok(Some(thrift::CandidateSelectionHint::bookmark_descendant(
            bookmark.to_string(),
        )))
    } else {
        Ok(None)
    }
}

pub(super) async fn run(
    matches: &ArgMatches<'_>,
    connection: Connection,
) -> Result<RenderStream, Error> {
    let source_repo = get_repo_specifier(matches, ARG_SOURCE_REPO)
        .ok_or(format_err!("repository is required"))?;
    let target_repo = get_repo_specifier(matches, ARG_TARGET_REPO)
        .ok_or(format_err!("repository is required"))?;

    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &source_repo, &commit_id).await?;
    let hint = build_hint(matches, &connection, &target_repo).await?;

    let commit = thrift::CommitSpecifier {
        repo: source_repo,
        id,
        ..Default::default()
    };
    let params = thrift::CommitLookupXRepoParams {
        other_repo: target_repo,
        identity_schemes: get_request_schemes(matches),
        candidate_selection_hint: hint,
        ..Default::default()
    };
    let response = connection.commit_lookup_xrepo(&commit, &params).await?;
    let ids = match &response.ids {
        Some(ids) => map_commit_ids(ids.values()),
        None => BTreeMap::new(),
    };

    let output = Box::new(LookupOutput {
        requested: commit_id.to_string(),
        exists: response.exists,
        ids,
    });

    Ok(stream::once(async move { Ok(output as Box<dyn Render>) }).boxed())
}

fn get_repo_specifier(matches: &ArgMatches, arg_name: &str) -> Option<thrift::RepoSpecifier> {
    matches
        .value_of(arg_name)
        .map(|name| thrift::RepoSpecifier {
            name: name.to_string(),
            ..Default::default()
        })
}
