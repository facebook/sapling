// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate blobrepo;
extern crate changesets;
extern crate clap;
#[macro_use]
extern crate cloned;
extern crate cmdlib;
extern crate context;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate mononoke_types;
#[macro_use]
extern crate slog;
extern crate tokio;

use std::cmp;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use clap::{App, Arg};
use failure::{Error, Result};
use futures::{stream, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;
use tokio::prelude::stream::iter_ok;

use blobrepo::alias::get_sha256;
use blobrepo::BlobRepo;
use changesets::SqlChangesets;
use cmdlib::args;
use context::CoreContext;
use mononoke_types::{hash::Sha256, ChangesetId, ContentId, FileChange, RepositoryId};

#[derive(Debug, Clone)]
enum Mode {
    Verify,
    Generate,
}

/// We are creating a separate object for SqlChangeset access, as we have added a specific
/// function to get all the ChangesetId. It is not a part of Changesets trait.
/// But blobrepo could provide us with Changesets object.
#[derive(Clone)]
struct AliasVerification {
    logger: Logger,
    blobrepo: Arc<BlobRepo>,
    repoid: RepositoryId,
    sqlchangesets: Arc<SqlChangesets>,
    mode: Mode,
    err_cnt: Arc<AtomicUsize>,
    cs_processed: Arc<AtomicUsize>,
}

impl AliasVerification {
    pub fn new(
        logger: Logger,
        blobrepo: Arc<BlobRepo>,
        repoid: RepositoryId,
        sqlchangesets: Arc<SqlChangesets>,
        mode: Mode,
    ) -> Self {
        Self {
            logger,
            blobrepo,
            repoid,
            sqlchangesets,
            mode,
            err_cnt: Arc::new(AtomicUsize::new(0)),
            cs_processed: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn get_file_changes_vector(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
    ) -> BoxFuture<Vec<Option<FileChange>>, Error> {
        let cs_cnt = self.cs_processed.fetch_add(1, Ordering::Relaxed);

        if cs_cnt % 1000 == 0 {
            info!(self.logger, "Commit processed {:?}", cs_cnt);
        }

        self.blobrepo
            .get_bonsai_changeset(ctx, bcs_id)
            .map(|bcs| {
                let file_changes: Vec<_> = bcs
                    .file_changes()
                    .map(|(_, file_change)| file_change.cloned())
                    .collect();
                file_changes
            })
            .boxify()
    }

    fn check_alias_blob(
        &self,
        alias: Sha256,
        expected_content_id: ContentId,
        content_id: ContentId,
    ) -> impl Future<Item = (), Error = Error> {
        if content_id == expected_content_id {
            // Everything is good
            Ok(()).into_future()
        } else {
            panic!(
                "Collision: Wrong content_id by alias for {:?},
                ContentId in the blobstore {:?},
                Expected ContentId {:?}",
                alias, content_id, expected_content_id
            );
        }
    }

    fn process_missing_alias_blob(
        &self,
        ctx: CoreContext,
        alias: Sha256,
        content_id: ContentId,
    ) -> impl Future<Item = (), Error = Error> {
        cloned!(self.blobrepo, self.logger, self.err_cnt, self.mode);

        err_cnt.fetch_add(1, Ordering::Relaxed);
        debug!(
            logger,
            "Missing alias blob: alias {:?}, content_id {:?}", alias, content_id
        );

        match mode {
            Mode::Verify => Ok(()).into_future().left_future(),
            Mode::Generate => blobrepo
                .upload_alias_to_file_content_id(ctx, alias, content_id)
                .right_future(),
        }
    }

    fn process_alias(
        &self,
        ctx: CoreContext,
        alias: Sha256,
        content_id: ContentId,
    ) -> impl Future<Item = (), Error = Error> {
        let av = self.clone();
        self.blobrepo
            .get_file_content_id_by_alias(ctx.clone(), alias)
            .then(move |result| match result {
                Ok(content_id_from_blobstore) => av
                    .check_alias_blob(alias, content_id, content_id_from_blobstore)
                    .left_future(),
                Err(_) => {
                    // the blob with alias is not found
                    av.process_missing_alias_blob(ctx, alias, content_id)
                        .right_future()
                }
            })
    }

    pub fn process_file_content(
        &self,
        ctx: CoreContext,
        content_id: ContentId,
    ) -> impl Future<Item = (), Error = Error> {
        let repo = self.blobrepo.clone();
        let av = self.clone();

        repo.get_file_content_by_content_id(ctx.clone(), content_id)
            .map(|content| get_sha256(&content.into_bytes()))
            .and_then(move |alias| av.process_alias(ctx, alias, content_id))
    }

    fn print_report(&self, partial: bool) {
        let av = self.clone();
        let resolution = if partial { "continues" } else { "finished" };

        info!(
            av.logger,
            "Alias Verification {}: {:?} errors found",
            resolution,
            av.err_cnt.load(Ordering::Relaxed)
        );
    }

    fn get_bounded(
        &self,
        ctx: CoreContext,
        min_id: u64,
        max_id: u64,
    ) -> impl Future<Item = (), Error = Error> {
        let av = self.clone();
        let av_for_process = self.clone();
        let av_for_report = self.clone();

        info!(
            self.logger,
            "Process Changesets with ids: [{:?}, {:?})", min_id, max_id
        );
        self.sqlchangesets
            // stream of cs_id
            .get_list_bs_cs_id_in_range(self.repoid, min_id, max_id)
            // future of vectors of file changes
            .map({
                cloned!(ctx);
                move |bcs_id| av.get_file_changes_vector(ctx.clone(), bcs_id)
            })
            .buffer_unordered(1000)
            // Stream of file_changes
            .map( move |file_changes_vec| {
                Ok(file_changes_vec)
                    .into_future()
                    .map(|file_changes| file_changes.into_iter())
                    .map(iter_ok)
                    .flatten_stream()
                }
            )
            .flatten()
            .map(move |file_change| {
                if let Some(file_change) = file_change {
                    let content_id = file_change.content_id().clone();
                    av_for_process
                        .process_file_content(ctx.clone(), content_id)
                        .left_future()
                } else {
                    Ok(()).into_future().right_future()
                }
            })
            .buffer_unordered(1000)
            .for_each(|()| Ok(()))
            .map(move |()| av_for_report.print_report(true))
    }

    pub fn verify_all(
        &self,
        ctx: CoreContext,
        step: u64,
        min_cs_db_id: u64,
    ) -> impl Future<Item = (), Error = Error> {
        let av = self.clone();
        let av_for_report = self.clone();

        self.sqlchangesets
            .get_changesets_ids_bounds(self.repoid)
            .map(move |(min_id, max_id)| {
                let mut bounds = vec![];

                let mut cur_id = cmp::max(min_id.unwrap(), min_cs_db_id);
                let max_id = max_id.unwrap() + 1;

                while cur_id < max_id {
                    let max = cmp::min(max_id, cur_id + step);
                    bounds.push((cur_id, max));
                    cur_id += step;
                }
                stream::iter_ok(bounds.into_iter())
            })
            .flatten_stream()
            .and_then(move |(min_val, max_val)| av.get_bounded(ctx.clone(), min_val, max_val))
            .for_each(|()| Ok(()))
            .map(move |()| av_for_report.print_report(false))
    }
}

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        safe_writes: true,
        hide_advanced_args: false,
        local_instances: true,
        default_glog: true,
    };
    app.build("Verify and reload all the alias blobs")
        .version("0.0.0")
        .about("Verify and reload all the alias blobs into Mononoke blobstore.")
        .arg(
            Arg::with_name("mode")
                .long("mode")
                .value_name("MODE")
                .possible_values(&["verify", "generate"])
                .default_value("verify")
                .help("mode for missing blobs"),
        )
        .arg(
            Arg::with_name("step")
                .long("step")
                .value_name("STEP")
                .default_value("5000")
                .help("Number of commit ids to process at a time"),
        )
        .arg(
            Arg::with_name("min-cs-db-id")
                .long("min-cs-db-id")
                .value_name("min_cs_db_id")
                .default_value("0")
                .help("Changeset to start verification from. Id from changeset table. Not connected to hash"),
        )
}

fn main() -> Result<()> {
    let matches = setup_app().get_matches();

    let ctx = CoreContext::test_mock();
    let logger = args::get_logger(&matches);

    args::init_cachelib(&matches);
    let sqlchangesets = Arc::new(args::open_sql::<SqlChangesets>(&matches)?);

    let mode = match matches.value_of("mode").expect("no default on mode") {
        "verify" => Mode::Verify,
        "generate" => Mode::Generate,
        bad => panic!("bad mode {}", bad),
    };
    let step = matches
        .value_of("step")
        .unwrap()
        .parse()
        .expect("Step should be numeric");
    let min_cs_db_id = matches
        .value_of("min-cs-db-id")
        .unwrap()
        .parse()
        .expect("Minimum Changeset Id should be numeric");

    let repoid = args::get_repo_id(&matches).expect("Need repo id");

    let blobrepo = args::open_repo(&logger, &matches);
    let aliasimport = blobrepo.and_then(move |blobrepo| {
        let blobrepo = Arc::new(blobrepo);
        AliasVerification::new(logger, blobrepo, repoid, sqlchangesets, mode).verify_all(
            ctx,
            step,
            min_cs_db_id,
        )
    });

    let mut runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(aliasimport);
    // Let the runtime finish remaining work - uploading logs etc
    runtime.shutdown_on_idle();
    result
}
