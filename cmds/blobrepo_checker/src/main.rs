// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bookmarks::Bookmark;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure_ext::Error;
use futures::{future, stream::futures_unordered, sync::mpsc, Future, Sink, Stream};
use std::fmt::Debug;
use tokio;

mod errors;

mod checks;
use crate::checks::{bonsai_checker_task, content_checker_task};

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
        .map(|bookmark| Bookmark::new(bookmark).expect("Bad bookmark to verify"))
        .collect();

    // No commit should have more than 2 parents, so this means no waiting at send time
    let (bonsai_to_check_sender, bonsai_to_check_receiver) = mpsc::channel(1);
    // File lists can be big - put backpressure on the sender if the receiver isn't keeping up.
    let (content_to_check_sender, content_to_check_receiver) = mpsc::channel(0);
    // As each commit and/or file can only generate one error, this is effectively unbounded
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

                tokio::spawn(
                    to_check
                        .forward(
                            bonsai_to_check_sender
                                .clone()
                                .sink_map_err(|_| panic!("Checker failed")),
                        )
                        .map(|_| ()),
                );

                tokio::spawn(bonsai_checker_task(
                    ctx.clone(),
                    repo.clone(),
                    bonsai_to_check_sender,
                    content_to_check_sender,
                    bonsai_to_check_receiver,
                    error_sender.clone(),
                ));

                tokio::spawn(content_checker_task(
                    ctx,
                    repo,
                    content_to_check_receiver,
                    error_sender,
                ));

                print_errors(error_receiver)
            }),
    )
}
