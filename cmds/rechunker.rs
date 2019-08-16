// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use clap::Arg;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{err_msg, Error};
use futures::future::Future;
use futures::stream;
use futures::stream::Stream;
use mercurial_types::{HgFileNodeId, HgNodeHash};
use std::str::FromStr;
use tokio;

use cmdlib::args;

const NAME: &str = "rechunker";
const DEFAULT_NUM_JOBS: usize = 10;

fn main() -> Result<(), Error> {
    let app = args::MononokeApp {
        safe_writes: false,
        hide_advanced_args: true,
        default_glog: false,
    };

    let matches = app
        .build(NAME)
        .version("0.0.0")
        .about("Rechunk blobs using the filestore")
        .arg(
            Arg::with_name("filenodes")
                .value_name("FILENODES")
                .takes_value(true)
                .required(true)
                .min_values(1)
                .help("filenode IDs for blobs to be rechunked"),
        )
        .arg(
            Arg::with_name("jobs")
                .short("j")
                .long("jobs")
                .value_name("JOBS")
                .takes_value(true)
                .help("The number of filenodes to rechunk in parallel"),
        )
        .get_matches();

    args::init_cachelib(&matches);

    let logger = args::get_logger(&matches);
    let ctx = CoreContext::test_mock();
    let blobrepo = args::open_repo(&logger, &matches);

    let jobs: usize = matches
        .value_of("jobs")
        .map_or(Ok(DEFAULT_NUM_JOBS), |j| j.parse())
        .map_err(Error::from)?;

    let filenode_ids: Vec<_> = matches
        .values_of("filenodes")
        .unwrap()
        .into_iter()
        .map(|f| {
            HgNodeHash::from_str(f)
                .map(HgFileNodeId::new)
                .map_err(|e| err_msg(format!("Invalid Sha1: {}", e)))
        })
        .collect();

    let rechunk = blobrepo.and_then(move |blobrepo| {
        stream::iter_result(filenode_ids)
            .map({
                cloned!(blobrepo);
                move |fid| {
                    blobrepo
                        .get_file_envelope(ctx.clone(), fid)
                        .map(|env| env.content_id())
                        .and_then({
                            cloned!(blobrepo, ctx);
                            move |content_id| blobrepo.rechunk_file_by_content_id(ctx, content_id)
                        })
                }
            })
            .buffered(jobs)
            .for_each(|_| Ok(()))
    });

    let mut runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(rechunk);
    runtime.shutdown_on_idle();
    result
}
