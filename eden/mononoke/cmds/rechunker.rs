/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use blobstore::PutBehaviour;
use clap_old::Arg;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream;
use futures::stream::TryStreamExt;

use mercurial_types::HgFileNodeId;
use mercurial_types::HgNodeHash;
use std::str::FromStr;

use cmdlib::args;
use cmdlib::helpers::block_execute;

const NAME: &str = "rechunker";
const DEFAULT_NUM_JOBS: usize = 10;

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = args::MononokeAppBuilder::new(NAME)
        .with_advanced_args_hidden()
        .with_special_put_behaviour(PutBehaviour::Overwrite)
        .build()
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
        .get_matches(fb)?;

    let logger = matches.logger();
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

    let blobrepo = args::open_repo(fb, logger, &matches);
    let rechunk = async move {
        let blobrepo: BlobRepo = blobrepo.await?;
        stream::iter(filenode_ids)
            .try_for_each_concurrent(jobs, |fid| {
                cloned!(blobrepo, ctx);
                async move {
                    let env = fid.load(&ctx, blobrepo.blobstore()).await?;
                    let content_id = env.content_id();
                    filestore::force_rechunk(
                        &blobrepo.get_blobstore(),
                        blobrepo.filestore_config().clone(),
                        &ctx,
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
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
