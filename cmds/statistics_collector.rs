// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::{App, Arg};
use cmdlib::args;
use context::CoreContext;
use failure::err_msg;
use failure_ext::Error;
use futures::future;
use futures::future::Future;
use futures::stream::Stream;
use manifest::ManifestOps;
use mercurial_types::Changeset;
use mercurial_types::HgChangesetId;
use std::str::FromStr;
use std::sync::Arc;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        hide_advanced_args: false,
    };
    app.build("Tool to calculate repo statistic")
        .version("0.0.0")
        .arg(
            Arg::with_name("changeset")
                .long("changeset")
                .required(true)
                .takes_value(true)
                .value_name("CHANGESET")
                .help("hg changeset hash"),
        )
}

#[derive(Clone, Copy)]
pub struct RepoStatistics {
    num_files: u64,
    total_file_size: u64,
}

impl RepoStatistics {
    pub fn new(num_files: u64, total_file_size: u64) -> Self {
        Self {
            num_files,
            total_file_size,
        }
    }
}

fn main() -> Result<(), Error> {
    let matches = setup_app().get_matches();

    args::init_cachelib(&matches);

    let logger = args::init_logging(&matches);
    let ctx = CoreContext::new_with_logger(logger.clone());

    let changeset = matches
        .value_of("changeset")
        .ok_or(err_msg("required parameter `changeset` is not set"))?;
    let changeset = HgChangesetId::from_str(changeset)?;

    let repo_statistics = args::open_repo(&logger, &matches).and_then(move |repo| {
        let blobstore = Arc::new(repo.get_blobstore());
        repo.get_changeset_by_changesetid(ctx.clone(), changeset.clone())
            .map(move |changeset| changeset.manifestid())
            .and_then(move |manifest_id| {
                manifest_id
                    .list_leaf_entries(ctx.clone(), blobstore.clone())
                    .map(move |(_, file_id)| repo.get_file_size(ctx.clone(), file_id.1))
                    .buffered(100)
                    .fold(RepoStatistics::new(0, 0), |mut statistics, file_size| {
                        statistics.num_files += 1;
                        statistics.total_file_size += file_size;
                        future::ok::<_, Error>(statistics)
                    })
            })
    });

    let mut runtime = tokio::runtime::Runtime::new()?;
    let repo_statistics = runtime.block_on(repo_statistics)?;
    runtime.shutdown_on_idle();

    println!(
        "Number of files: {}, total file size: {}",
        repo_statistics.num_files, repo_statistics.total_file_size
    );
    Ok(())
}
