// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::{App, Arg};
use cmdlib::args;
use failure::err_msg;
use failure_ext::Error;
use futures::prelude::*;
use manifest::ManifestOps;
use mercurial_types::Changeset;
use mercurial_types::HgChangesetId;
use std::str::FromStr;
use std::sync::Arc;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        hide_advanced_args: false,
        default_glog: true,
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

fn main() -> Result<(), Error> {
    let matches = setup_app().get_matches();

    let ctx = args::get_core_context(&matches);

    args::init_cachelib(&matches);

    let logger = args::get_logger(&matches);

    let changeset = matches
        .value_of("changeset")
        .ok_or(err_msg("required parameter `changeset` is not set"))?;
    let changeset = HgChangesetId::from_str(changeset)?;

    let files_number = args::open_repo(&logger, &matches).and_then(move |repo| {
        let blobstore = Arc::new(repo.get_blobstore());
        repo.get_changeset_by_changesetid(ctx.clone(), changeset.clone())
            .map(|changeset| changeset.manifestid())
            .and_then(move |manifest_id| {
                manifest_id
                    .list_leaf_entries(ctx.clone(), blobstore.clone())
                    .collect()
            })
            .map(|leaf_list| leaf_list.len())
    });
    let mut runtime = tokio::runtime::Runtime::new()?;
    let files_number = runtime.block_on(files_number)?;
    runtime.shutdown_on_idle();

    println!("Number of files: {}", files_number);
    Ok(())
}
