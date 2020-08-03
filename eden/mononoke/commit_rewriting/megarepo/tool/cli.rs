/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use bookmarks::BookmarkName;
use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::args;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use futures_old::future::{err, ok};
use megarepolib::common::{ChangesetArgs, ChangesetArgsFactory, StackPosition};
use mononoke_types::DateTime;

pub const COMMIT_HASH: &'static str = "commit-hash";
pub const MOVE: &'static str = "move";
pub const MERGE: &'static str = "merge";
pub const MARK_PUBLIC: &'static str = "mark-public";
pub const ORIGIN_REPO: &'static str = "origin-repo";
pub const CHANGESET: &'static str = "commit";
pub const FIRST_PARENT: &'static str = "first-parent";
pub const SECOND_PARENT: &'static str = "second-parent";
pub const COMMIT_MESSAGE: &'static str = "commit-message";
pub const COMMIT_AUTHOR: &'static str = "commit-author";
pub const COMMIT_DATE_RFC3339: &'static str = "commit-date-rfc3339";
pub const COMMIT_BOOKMARK: &'static str = "bookmark";
pub const SYNC_DIAMOND_MERGE: &'static str = "sync-diamond-merge";
pub const MAX_NUM_OF_MOVES_IN_COMMIT: &'static str = "max-num-of-moves-in-commit";
pub const CHUNKING_HINT_FILE: &'static str = "chunking-hint-file";
pub const PRE_MERGE_DELETE: &'static str = "pre-merge-delete";
pub const EVEN_CHUNK_SIZE: &'static str = "even-chunk-size";

pub fn cs_args_from_matches<'a>(sub_m: &ArgMatches<'a>) -> BoxFuture<ChangesetArgs, Error> {
    let message = try_boxfuture!(sub_m
        .value_of(COMMIT_MESSAGE)
        .ok_or_else(|| format_err!("missing argument {}", COMMIT_MESSAGE)))
    .to_string();
    let author = try_boxfuture!(sub_m
        .value_of(COMMIT_AUTHOR)
        .ok_or_else(|| format_err!("missing argument {}", COMMIT_AUTHOR)))
    .to_string();
    let datetime = try_boxfuture!(sub_m
        .value_of(COMMIT_DATE_RFC3339)
        .map(|datetime_str| DateTime::from_rfc3339(datetime_str))
        .unwrap_or_else(|| Ok(DateTime::now())));
    let bookmark = try_boxfuture!(sub_m
        .value_of(COMMIT_BOOKMARK)
        .map(|bookmark_str| BookmarkName::new(bookmark_str))
        .transpose());
    let mark_public = sub_m.is_present(MARK_PUBLIC);
    if !mark_public && bookmark.is_some() {
        return err(format_err!(
            "--mark-public is required if --bookmark is provided"
        ))
        .boxify();
    }

    ok(ChangesetArgs {
        author,
        message,
        datetime,
        bookmark,
        mark_public,
    })
    .boxify()
}

pub fn get_delete_commits_cs_args_factory<'a>(
    sub_m: &ArgMatches<'a>,
) -> Result<Box<dyn ChangesetArgsFactory>, Error> {
    let message = sub_m
        .value_of(COMMIT_MESSAGE)
        .ok_or_else(|| format_err!("missing argument {}", COMMIT_MESSAGE))?
        .to_string();

    let author = sub_m
        .value_of(COMMIT_AUTHOR)
        .ok_or_else(|| format_err!("missing argument {}", COMMIT_AUTHOR))?
        .to_string();

    let datetime = sub_m
        .value_of(COMMIT_DATE_RFC3339)
        .map(|datetime_str| DateTime::from_rfc3339(datetime_str))
        .transpose()?
        .unwrap_or_else(|| DateTime::now());

    Ok(Box::new(move |num: StackPosition| ChangesetArgs {
        author: author.clone(),
        message: format!("[MEGAREPO DELETE] {} ({})", message, num.0),
        datetime: datetime.clone(),
        bookmark: None,
        mark_public: false,
    }))
}

fn add_resulting_commit_args<'a, 'b>(subcommand: App<'a, 'b>) -> App<'a, 'b> {
    subcommand
        .arg(
            Arg::with_name(COMMIT_AUTHOR)
                .help("commit author to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(COMMIT_MESSAGE)
                .help("commit message to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(MARK_PUBLIC)
                .help("add the resulting commit to the public phase")
                .long(MARK_PUBLIC),
        )
        .arg(
            Arg::with_name(COMMIT_DATE_RFC3339)
                .help("commit date to use (default is now)")
                .long(COMMIT_DATE_RFC3339)
                .takes_value(true),
        )
        .arg(
            Arg::with_name(COMMIT_BOOKMARK)
                .help("bookmark to point to resulting commits (no sanity checks, will move existing bookmark, be careful)")
                .long(COMMIT_BOOKMARK)
                .takes_value(true)
        )
}

fn add_light_resulting_commit_args<'a, 'b>(subcommand: App<'a, 'b>) -> App<'a, 'b> {
    subcommand
        .arg(
            Arg::with_name(COMMIT_AUTHOR)
                .help("commit author to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(COMMIT_MESSAGE)
                .help("commit message to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(COMMIT_DATE_RFC3339)
                .help("commit date to use (default is now)")
                .long(COMMIT_DATE_RFC3339)
                .takes_value(true),
        )
}

pub fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let move_subcommand = SubCommand::with_name(MOVE)
        .about("create a move commit, using a provided spec")
        .arg(
            Arg::with_name(MAX_NUM_OF_MOVES_IN_COMMIT)
                .long(MAX_NUM_OF_MOVES_IN_COMMIT)
                .help("how many files a single commit moves (note - that might create a stack of move commits instead of just one)")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ORIGIN_REPO)
                .help("use predefined mover for part of megarepo, coming from this repo")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(CHANGESET)
                .help("a changeset hash or bookmark of move commit's parent")
                .takes_value(true)
                .required(true),
        );

    let merge_subcommand = SubCommand::with_name(MERGE)
        .about("create a merge commit with given parents")
        .arg(
            Arg::with_name(FIRST_PARENT)
                .help("first parent of a produced merge commit")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(SECOND_PARENT)
                .help("second parent of a produced merge commit")
                .takes_value(true)
                .required(true),
        );

    let sync_diamond_subcommand = SubCommand::with_name(SYNC_DIAMOND_MERGE)
        .about("sync a diamond merge commit from a small repo into large repo")
        .arg(
            Arg::with_name(COMMIT_HASH)
                .help("diamond merge commit from small repo to sync")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(COMMIT_BOOKMARK)
                .help("bookmark to point to resulting commits (no sanity checks, will move existing bookmark, be careful)")
                .long(COMMIT_BOOKMARK)
                .takes_value(true)
        );

    let pre_merge_delete_subcommand = SubCommand::with_name(PRE_MERGE_DELETE)
        .about("create a set of pre-merge delete commtis, as well as commits to merge into the target branch")
        .arg(
            Arg::with_name(COMMIT_HASH)
                .help("commit from which to start deletion")
                .takes_value(true)
                .required(true)
        )
        .arg(
            Arg::with_name(CHUNKING_HINT_FILE)
                .help(r#"a path to working copy chunking hint. If not provided, working copy will
                        be chunked evenly into `--even-chunk-size` commits"#)
                .long(CHUNKING_HINT_FILE)
                .takes_value(true)
                .required(false)
        )
        .arg(
            Arg::with_name(EVEN_CHUNK_SIZE)
                .help("chunk size for even chunking when --chunking-hing-file is not provided")
                .long(EVEN_CHUNK_SIZE)
                .takes_value(true)
                .required(false)
        );

    args::MononokeApp::new("megarepo preparation tool")
        .with_advanced_args_hidden()
        .with_source_and_target_repos()
        .build()
        .subcommand(add_resulting_commit_args(move_subcommand))
        .subcommand(add_resulting_commit_args(merge_subcommand))
        .subcommand(sync_diamond_subcommand)
        .subcommand(add_light_resulting_commit_args(pre_merge_delete_subcommand))
}
