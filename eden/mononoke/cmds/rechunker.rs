/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{format_err, Error};
use blobstore::Loadable;
use clap::Arg;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    stream::{self, TryStreamExt},
};

use mercurial_types::{HgFileNodeId, HgNodeHash};
use std::str::FromStr;

use cmdlib::{args, helpers::block_execute};

const NAME: &str = "rechunker";
const DEFAULT_NUM_JOBS: usize = 10;

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = args::MononokeApp::new(NAME)
        .with_advanced_args_hidden()
        .build()
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

    args::init_cachelib(fb, &matches, None);

    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

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
                .map_err(|e| format_err!("Invalid Sha1: {}", e))
        })
        .collect();

    let blobrepo = args::open_repo(fb, &logger, &matches);
    let rechunk = async move {
        let blobrepo = blobrepo.compat().await?;
        stream::iter(filenode_ids)
            .try_for_each_concurrent(jobs, |fid| {
                cloned!(blobrepo, ctx);
                async move {
                    let env = fid.load(ctx.clone(), blobrepo.blobstore()).compat().await?;
                    let content_id = env.content_id();
                    filestore::force_rechunk(
                        blobrepo.get_blobstore(),
                        blobrepo.filestore_config().clone(),
                        ctx,
                        content_id,
                    )
                    .await
                    .map(|_| ())
                }
            })
            .await
    };

    block_execute(
        rechunk,
        fb,
        "rechunker",
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
