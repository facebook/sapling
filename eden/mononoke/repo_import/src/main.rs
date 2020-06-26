/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use clap::Arg;
use cmdlib::args;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use git2::Oid;
use import_tools::{GitimportPreferences, GitimportTarget};
use linked_hash_map::LinkedHashMap;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use std::path::Path;

const ARG_GIT_REPOSITORY_PATH: &str = "git-repository-path";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Import Repository")
        .with_advanced_args_hidden()
        .build()
        .version("0.0.0")
        .about("Automating repository imports")
        .arg(Arg::with_name(ARG_GIT_REPOSITORY_PATH).help("Path to a git repository to import"));

    let prefs = GitimportPreferences::default();
    let matches = app.get_matches();

    let target = GitimportTarget::FullRepo;

    let path = Path::new(matches.value_of(ARG_GIT_REPOSITORY_PATH).unwrap());
    args::init_cachelib(fb, &matches, None);

    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::create_repo(fb, &logger, &matches);

    let gitimport_result: Result<LinkedHashMap<Oid, (ChangesetId, BonsaiChangeset)>, Error> =
        block_execute(
            async {
                let repo = repo.compat().await?;
                import_tools::gitimport(&ctx, &repo, &path, target, prefs).await
            },
            fb,
            "gitimport",
            &logger,
            &matches,
            cmdlib::monitoring::AliveService,
        );

    match gitimport_result {
        Ok(_import_map) => Ok(()),
        Err(e) => Err(e),
    }
}
