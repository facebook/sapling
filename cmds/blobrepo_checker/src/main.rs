/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use bookmarks::BookmarkName;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use failure_ext::Error;
use fbinit::FacebookInit;
use futures::{future, stream::futures_unordered, sync::mpsc, Future, Stream};
use std::fmt::Debug;

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

#[fbinit::main]
fn main(fb: FacebookInit) {
    let matches = args::MononokeApp::new("Blobrepo checker")
        .with_advanced_args_hidden()
        .build()
        .version("0.0.0")
        .about("Validate that a blobrepo has all file content and history needed to check out a given commit or its ancestors")
        .args_from_usage(
            r#"
               <BOOKMARK>...                             'Bookmark whose history should be checked'
            "#,
        )
        .get_matches();

    args::init_cachelib(fb, &matches);

    let logger = args::init_logging(fb, &matches);

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let repo_fut = args::open_scrub_repo(fb, &logger, &matches);

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
