// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use bookmarks::{BookmarkName, BookmarkUpdateReason, Bookmarks};
use clap::{App, Arg, SubCommand};
use cmdlib::args;
use context::CoreContext;
use dbbookmarks::SqlBookmarks;
use failure_ext::Result;
use futures::future::Future;
use mononoke_types::ChangesetId;

const CREATE: &'static str = "create";
const UPDATE: &'static str = "update";

const BOOKMARK: &'static str = "bookmark";
const BLOBIMPORT: &'static str = "blobimport";

const FROM_ID: &'static str = "from_id";
const TO_ID: &'static str = "to_id";
const ID: &'static str = "id";

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let create = SubCommand::with_name(CREATE).arg(Arg::with_name(ID).required(true));

    let update = SubCommand::with_name(UPDATE)
        .arg(Arg::with_name(FROM_ID).required(true))
        .arg(Arg::with_name(TO_ID).required(true));

    args::MononokeApp {
        safe_writes: false,
        hide_advanced_args: true,
        default_glog: true,
    }
    .build("Insert stub log entries - use to test e.g. the admin tool")
    .version("0.0.0")
    .arg(
        Arg::with_name(BOOKMARK)
            .long(BOOKMARK)
            .takes_value(true)
            .required(true),
    )
    .arg(
        Arg::with_name(BLOBIMPORT)
            .long(BLOBIMPORT)
            .required(false)
            .help("Use blobimport reason"),
    )
    .subcommand(create)
    .subcommand(update)
}

fn main() -> Result<()> {
    let ctx = CoreContext::test_mock();

    let matches = setup_app().get_matches();

    let repo_id = args::get_repo_id(&matches).unwrap();
    let bookmarks = args::open_sql::<SqlBookmarks>(&matches).unwrap();

    let name = matches.value_of(BOOKMARK).unwrap().to_string();
    let reason = match matches.is_present(BLOBIMPORT) {
        true => BookmarkUpdateReason::Blobimport,
        false => BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    };

    let bookmark = BookmarkName::new(name).unwrap();

    let mut txn = bookmarks.create_transaction(ctx, repo_id);

    match matches.subcommand() {
        (CREATE, Some(sub_m)) => {
            txn.create(
                &bookmark,
                ChangesetId::from_str(&sub_m.value_of(ID).unwrap().to_string()).unwrap(),
                reason,
            )
            .unwrap();
        }
        (UPDATE, Some(sub_m)) => {
            txn.update(
                &bookmark,
                ChangesetId::from_str(&sub_m.value_of(TO_ID).unwrap().to_string()).unwrap(),
                ChangesetId::from_str(&sub_m.value_of(FROM_ID).unwrap().to_string()).unwrap(),
                reason,
            )
            .unwrap();
        }
        _ => {
            println!("{}", matches.usage());
            ::std::process::exit(1);
        }
    }

    let fut = txn.commit();

    tokio::run(fut.map(|_| ()).map_err(move |err| {
        println!("{:?}", err);
        ::std::process::exit(1);
    }));

    Ok(())
}
