// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bookmarks::BookmarkName;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure_ext::Error;
use futures::{future, stream::futures_unordered, sync::mpsc, Future, Stream};
use std::fmt::Debug;
use tokio;

mod errors;

mod checks;
use crate::checks::Checker;

fn print_errors<S, E>(error: S) -> impl Future<Item = (), Error = ()>
where
    S: Stream<Item = Error, Error = E>,
    E: Debug,
{
    error
        .for_each(|err| {
            eprintln!("{}", err);
            Ok(())
        })
        .map_err(|e| panic!("While printing errors: {:#?}", e))
}

fn main() {
    let app = args::MononokeApp {
        safe_writes: false,
        hide_advanced_args: true,
        local_instances: false,
        default_glog: false,
    };
    let matches = app
        .build("Blobrepo checker")
        .version("0.0.0")
        .about("Validate that a blobrepo has all file content and history needed to check out a given commit or its ancestors")
        .args_from_usage(
            r#"
               <BOOKMARK>...                             'Bookmark whose history should be checked'
            "#,
        )
        .get_matches();

    args::init_cachelib(&matches);

    let logger = args::get_logger(&matches);

    let ctx = CoreContext::test_mock();

    let repo_fut = args::open_repo(&logger, &matches);

    let bookmarks: Vec<_> = matches
        .values_of("BOOKMARK")
        .expect("No bookmarks to verify")
        .map(|bookmark| BookmarkName::new(bookmark).expect("Bad bookmark to verify"))
        .collect();

    let checkers = Checker::new();
    let (error_sender, error_receiver) = mpsc::channel(0);

    tokio::run(
        repo_fut
            .map_err(|e| eprintln!("Can't get repo: {:#?}", e))
            .and_then(move |repo| {
                let to_check = futures_unordered(bookmarks.into_iter().map(|bookmark| {
                    repo.get_bonsai_bookmark(ctx.clone(), &bookmark)
                        .and_then({
                            cloned!(bookmark);
                            move |opt| match opt {
                                None => panic!("Bookmark {} not found", bookmark),
                                Some(cs) => future::ok(cs),
                            }
                        })
                        .map_err(move |e| panic!("Bookmark {}: {:#?}", bookmark, e))
                }));

                tokio::spawn(checkers.queue_root_commits(to_check));

                checkers.spawn_tasks(ctx, repo, error_sender);

                print_errors(error_receiver)
            }),
    )
}
