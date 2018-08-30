// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::{App, Arg, ArgMatches, SubCommand};
use failure::Error;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;

use blobrepo::BlobRepo;
use bookmarks::Bookmark;

const SET_CMD: &'static str = "set";
const GET_CMD: &'static str = "get";

pub fn prepare_command<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    let set = SubCommand::with_name(SET_CMD)
        .about(
            "sets a bookmark to a specific hg changeset, if the bookmark does not exist it will
                be created",
        )
        .args_from_usage(
            "<BOOKMARK_NAME>        'bookmark to target'
             <HG_CHANGESET_ID>      'revision to which the bookmark should point to'",
        );

    let get = SubCommand::with_name(GET_CMD)
        .about("gets the changeset of a specific bookmark")
        .args_from_usage("<BOOKMARK_NAME>        'bookmark to target'")
        .arg(
            Arg::with_name("changeset-type")
                .long("changeset-type")
                .short("cs")
                .takes_value(true)
                .possible_values(&["bonsai", "hg"])
                .required(false)
                .help("What changeset type to return, either bonsai or hg. Defaults to hg."),
        );

    app.about("set of commands to manipulate bookmarks")
        .subcommand(set)
        .subcommand(get)
}

pub fn handle_command<'a>(
    repo: &BlobRepo,
    matches: &ArgMatches<'a>,
    logger: Logger,
) -> BoxFuture<(), Error> {
    match matches.subcommand() {
        (GET_CMD, Some(sub_m)) => handle_get(sub_m, logger, repo.clone()),
        (SET_CMD, Some(sub_m)) => handle_set(sub_m, logger, repo.clone()),
        _ => {
            println!("{}", matches.usage());
            ::std::process::exit(1);
        }
    }
}

fn handle_get<'a>(args: &ArgMatches<'a>, _logger: Logger, repo: BlobRepo) -> BoxFuture<(), Error> {
    let bookmark_name = args.value_of("BOOKMARK_NAME").unwrap().to_string();
    let bookmark = Bookmark::new(bookmark_name).unwrap();
    let changeset_type = args.value_of("changeset-type").unwrap_or("hg");

    match changeset_type {
        "hg" => repo.get_bookmark(&bookmark)
            .and_then(move |cs| {
                println!("(HG) {}", cs.expect("bookmark could not be found"));
                future::ok(())
            })
            .boxify(),

        "bonsai" => {
            cloned!(bookmark);
            ::fetch_bonsai_changeset(bookmark.to_string().as_str(), &repo)
                .and_then(move |bonsai_cs| {
                    println!("(BONSAI) {}", bonsai_cs.get_changeset_id());
                    future::ok(())
                })
                .boxify()
        }

        _ => panic!("Unknown changeset-type supplied"),
    }
}

fn handle_set<'a>(args: &ArgMatches<'a>, _logger: Logger, repo: BlobRepo) -> BoxFuture<(), Error> {
    let bookmark_name = args.value_of("BOOKMARK_NAME").unwrap().to_string();
    let rev = args.value_of("HG_CHANGESET_ID").unwrap();
    let bookmark = Bookmark::new(bookmark_name).unwrap();

    ::fetch_bonsai_changeset(rev, &repo)
        .and_then(move |bonsai_cs| {
            let mut transaction = repo.update_bookmark_transaction();
            try_boxfuture!(transaction.force_set(&bookmark, &bonsai_cs.get_changeset_id()));
            transaction.commit().map(|_| ()).from_err().boxify()
        })
        .boxify()
}
