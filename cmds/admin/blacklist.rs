// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::ArgMatches;
use cmdlib::args;

use crate::common::{get_file_nodes, resolve_hg_rev};
use censoredblob::SqlCensoredContentStore;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{format_err, Error, FutureFailureErrorExt};
use futures::future::{join_all, Future};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use mercurial_types::MPath;
use mononoke_types::{typed_hash::MononokeId, ContentId, Timestamp};
use slog::{debug, Logger};

use crate::error::SubcommandError;

pub fn subcommand_blacklist(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let rev = sub_m.value_of("hash").unwrap().to_string();
    let task = sub_m.value_of("task").unwrap().to_string();

    let paths: Result<Vec<_>, Error> = sub_m
        .values_of("FILES_LIST")
        .expect("at least one file")
        .map(|path| {
            let mpath = MPath::new(path);
            match mpath {
                Ok(mpath) => Ok(mpath),
                Err(_) => Err(format_err!(
                    "The following path could not be parsed {}",
                    path
                )),
            }
        })
        .collect();

    let paths: Vec<_> = try_boxfuture!(paths);

    let ctx = CoreContext::test_mock();
    args::init_cachelib(&matches);

    let censored_blobs = args::open_sql::<SqlCensoredContentStore>(&matches)
        .context("While opening SqlCensoredContentStore")
        .from_err();

    let blobrepo = args::open_repo(&logger, &matches);

    blobrepo
        .and_then({
            cloned!(ctx);
            move |blobrepo| {
                resolve_hg_rev(ctx.clone(), &blobrepo, &rev).map(|cs_id| (blobrepo, cs_id))
            }
        })
        .join(censored_blobs)
        .and_then({
            move |((blobrepo, cs_id), censored_blobs)| {
                get_file_nodes(ctx.clone(), logger.clone(), &blobrepo, cs_id, paths).and_then({
                    move |hg_node_ids| {
                        let content_ids = hg_node_ids.into_iter().map(move |hg_node_id| {
                            blobrepo.get_file_content_id(ctx.clone(), hg_node_id)
                        });

                        debug!(logger, "Inserting all the blobstore keys in the database");
                        join_all(content_ids).and_then(move |content_ids: Vec<ContentId>| {
                            let blobstore_keys = content_ids
                                .iter()
                                .map(|content_id| content_id.blobstore_key())
                                .collect();
                            let timestamp = Timestamp::now();
                            censored_blobs.insert_censored_blobs(&blobstore_keys, &task, &timestamp)
                        })
                    }
                })
            }
        })
        .from_err()
        .boxify()
}
