/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use clap_old::Arg;
use fbinit::FacebookInit;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use slog::debug;
use slog::info;
use slog::Logger;

use blobrepo::BlobRepo;
use blobstore::Loadable;
use blobstore::Storable;
use changesets::ChangesetsRef;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use filestore::Alias;
use filestore::AliasBlob;
use filestore::FetchKey;
use mercurial_types::FileBytes;
use mononoke_types::hash;
use mononoke_types::hash::Sha256;
use mononoke_types::ChangesetId;
use mononoke_types::ContentAlias;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::RepositoryId;

const LIMIT: usize = 1000;

pub fn get_sha256(contents: &Bytes) -> hash::Sha256 {
    use sha2::Digest;
    use sha2::Sha256;
    let mut hasher = Sha256::new();
    hasher.update(contents);
    hash::Sha256::from_byte_array(hasher.finalize().into())
}

#[derive(Debug, Clone)]
enum Mode {
    Verify,
    Generate,
}

#[derive(Clone)]
struct AliasVerification {
    logger: Logger,
    blobrepo: BlobRepo,
    #[allow(dead_code)]
    repoid: RepositoryId,
    mode: Mode,
    err_cnt: Arc<AtomicUsize>,
    cs_processed: Arc<AtomicUsize>,
}

impl AliasVerification {
    pub fn new(logger: Logger, blobrepo: BlobRepo, repoid: RepositoryId, mode: Mode) -> Self {
        Self {
            logger,
            blobrepo,
            repoid,
            mode,
            err_cnt: Arc::new(AtomicUsize::new(0)),
            cs_processed: Arc::new(AtomicUsize::new(0)),
        }
    }

    async fn get_file_changes_vector(
        &self,
        ctx: &CoreContext,
        bcs_id: ChangesetId,
    ) -> Result<Vec<FileChange>, Error> {
        let cs_cnt = self.cs_processed.fetch_add(1, Ordering::Relaxed);

        if cs_cnt % 1000 == 0 {
            info!(self.logger, "Commit processed {:?}", cs_cnt);
        }

        let bcs = bcs_id.load(ctx, self.blobrepo.blobstore()).await?;
        let file_changes: Vec<_> = bcs
            .file_changes_map()
            .iter()
            .map(|(_path, fc)| fc.clone())
            .collect();
        Ok(file_changes)
    }

    async fn check_alias_blob(
        &self,
        alias: &Sha256,
        expected_content_id: ContentId,
        content_id: ContentId,
    ) -> Result<(), Error> {
        if content_id == expected_content_id {
            // Everything is good
            Ok(())
        } else {
            panic!(
                "Collision: Wrong content_id by alias for {:?},
                ContentId in the blobstore {:?},
                Expected ContentId {:?}",
                alias, content_id, expected_content_id
            );
        }
    }

    async fn process_missing_alias_blob(
        &self,
        ctx: &CoreContext,
        alias: &Sha256,
        content_id: ContentId,
    ) -> Result<(), Error> {
        self.err_cnt.fetch_add(1, Ordering::Relaxed);
        debug!(
            self.logger,
            "Missing alias blob: alias {:?}, content_id {:?}", alias, content_id
        );

        match self.mode {
            Mode::Verify => Ok(()),
            Mode::Generate => {
                let blobstore = self.blobrepo.get_blobstore();

                let maybe_meta =
                    filestore::get_metadata(&blobstore, ctx, &FetchKey::Canonical(content_id))
                        .await?;

                let meta =
                    maybe_meta.ok_or_else(|| format_err!("Missing content {:?}", content_id))?;

                if meta.sha256 == *alias {
                    AliasBlob(
                        Alias::Sha256(meta.sha256),
                        ContentAlias::from_content_id(content_id),
                    )
                    .store(ctx, &blobstore)
                    .await
                } else {
                    Err(format_err!(
                        "Inconsistent hashes for {:?}, got {:?}, meta is {:?}",
                        content_id,
                        alias,
                        meta.sha256
                    ))
                }
            }
        }
    }

    async fn process_alias(
        &self,
        ctx: &CoreContext,
        alias: &Sha256,
        content_id: ContentId,
    ) -> Result<(), Error> {
        let result = FetchKey::from(alias.clone())
            .load(ctx, self.blobrepo.blobstore())
            .await;

        match result {
            Ok(content_id_from_blobstore) => {
                self.check_alias_blob(alias, content_id, content_id_from_blobstore)
                    .await
            }
            Err(_) => {
                // the blob with alias is not found
                self.process_missing_alias_blob(ctx, alias, content_id)
                    .await
            }
        }
    }

    pub async fn process_file_content(
        &self,
        ctx: &CoreContext,
        content_id: ContentId,
    ) -> Result<(), Error> {
        let repo = self.blobrepo.clone();

        let alias = filestore::fetch_concat(repo.blobstore(), ctx, content_id)
            .map_ok(FileBytes)
            .map_ok(|content| get_sha256(&content.into_bytes()))
            .await?;

        self.process_alias(ctx, &alias, content_id).await
    }

    fn print_report(&self, partial: bool) {
        let resolution = if partial { "continues" } else { "finished" };

        info!(
            self.logger,
            "Alias Verification {}: {:?} errors found",
            resolution,
            self.err_cnt.load(Ordering::Relaxed)
        );
    }

    async fn get_bounded(&self, ctx: &CoreContext, min_id: u64, max_id: u64) -> Result<(), Error> {
        info!(
            self.logger,
            "Process Changesets with ids: [{:?}, {:?})", min_id, max_id
        );

        let bcs_ids = self
            .blobrepo
            .changesets()
            .list_enumeration_range(ctx, min_id, max_id, None, true);

        bcs_ids
            .and_then(move |(bcs_id, _)| async move {
                let file_changes_vec = self.get_file_changes_vector(ctx, bcs_id).await?;
                Ok(stream::iter(file_changes_vec).map(Ok))
            })
            .try_flatten()
            .try_for_each_concurrent(LIMIT, move |file_change| async move {
                match file_change.simplify() {
                    Some(tc) => {
                        self.process_file_content(ctx, tc.content_id().clone())
                            .await
                    }
                    None => Ok(()),
                }
            })
            .await?;

        self.print_report(true);
        Ok(())
    }

    pub async fn verify_all(
        &self,
        ctx: &CoreContext,
        step: u64,
        min_cs_db_id: u64,
    ) -> Result<(), Error> {
        let (min_id, max_id) = self
            .blobrepo
            .changesets()
            .enumeration_bounds(ctx, true, vec![])
            .await?
            .unwrap();

        let mut bounds = vec![];
        let mut cur_id = cmp::max(min_id, min_cs_db_id);
        let max_id = max_id + 1;
        while cur_id < max_id {
            let max = cmp::min(max_id, cur_id + step);
            bounds.push((cur_id, max));
            cur_id += step;
        }

        stream::iter(bounds)
            .map(Ok)
            .try_for_each(move |(min_val, max_val)| self.get_bounded(ctx, min_val, max_val))
            .await?;

        self.print_report(false);
        Ok(())
    }
}

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("Verify and reload all the alias blobs")
        .build()
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

async fn run_aliasverify<'a>(
    fb: FacebookInit,
    ctx: CoreContext,
    logger: &Logger,
    step: u64,
    min_cs_db_id: u64,
    repoid: RepositoryId,
    matches: &'a MononokeMatches<'a>,
    mode: Mode,
) -> Result<(), Error> {
    let blobrepo = args::open_repo(fb, logger, matches).await?;
    AliasVerification::new(logger.clone(), blobrepo, repoid, mode)
        .verify_all(&ctx, step, min_cs_db_id)
        .await
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches(fb)?;

    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let config_store = matches.config_store();

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

    let repoid = args::get_repo_id(config_store, &matches).expect("Need repo id");

    block_execute(
        run_aliasverify(fb, ctx, logger, step, min_cs_db_id, repoid, &matches, mode),
        fb,
        "aliasverify",
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
