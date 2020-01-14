/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::validate::{CHECK_FAIL, CHECK_TYPE, NODE_KEY, REPO};

use anyhow::{format_err, Error};
use blobstore::Blobstore;
use blobstore_factory::{
    make_blobstore, make_blobstore_multiplexed, make_sql_factory, BlobstoreOptions,
    ReadOnlyStorage, SqlFactory,
};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    self,
    future::{self, Future},
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use inlinable_string::InlinableString;
use metaconfig_types::{BlobConfig, BlobstoreId, ScrubAction, StorageConfig};
use multiplexedblob::{LoggingScrubHandler, ScrubHandler};
use prefixblob::PrefixBlobstore;
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use sql_ext::MysqlOptions;
use stats::prelude::*;
use std::{convert::From, sync::Arc};

define_stats! {
    prefix = "mononoke.walker";
    scrub_repaired: dynamic_timeseries("{}.blobstore.{}.{}.repaired", (subcommand: &'static str, blobstore_id: String, repo: String); Rate, Sum),
    scrub_repair_required: dynamic_timeseries("{}.blobstore.{}.{}.repair_required", (subcommand: &'static str, blobstore_id: String, repo: String); Rate, Sum),
}

pub const BLOBSTORE_ID: &'static str = "blobstore_id";

pub struct StatsScrubHandler {
    scuba: ScubaSampleBuilder,
    subcommand_stats_key: &'static str,
    repo_stats_key: String,
    inner: LoggingScrubHandler,
}

impl StatsScrubHandler {
    pub fn new(
        quiet: bool,
        scuba: ScubaSampleBuilder,
        subcommand_stats_key: &'static str,
        repo_stats_key: String,
    ) -> Self {
        Self {
            scuba,
            subcommand_stats_key,
            repo_stats_key,
            inner: LoggingScrubHandler::new(quiet),
        }
    }
}

impl ScrubHandler for StatsScrubHandler {
    fn on_repair(
        &self,
        ctx: &CoreContext,
        blobstore_id: BlobstoreId,
        key: &str,
        is_repaired: bool,
    ) {
        self.inner.on_repair(ctx, blobstore_id, key, is_repaired);
        self.scuba.clone()
            // If we start to run in multi-repo mode this will need to be prefix aware instead
            .add(REPO, self.repo_stats_key.clone())
            .add(BLOBSTORE_ID, blobstore_id)
            // TODO parse out NodeType from string key prefix if we can. Or better, make blobstore keys typed?
            .add(NODE_KEY, key)
            .add(CHECK_TYPE, "scrub_repair")
            .add(
                CHECK_FAIL,
                if is_repaired {
                    0
                } else {
                    1
                },
            )
            .log();
        if is_repaired {
            STATS::scrub_repaired.add_value(
                1,
                (
                    self.subcommand_stats_key,
                    blobstore_id.to_string(),
                    self.repo_stats_key.clone(),
                ),
            );
        } else {
            STATS::scrub_repair_required.add_value(
                1,
                (
                    self.subcommand_stats_key,
                    blobstore_id.to_string(),
                    self.repo_stats_key.clone(),
                ),
            );
        }
    }
}

fn get_blobconfig(
    blob_config: BlobConfig,
    inner_blobstore_id: Option<u64>,
) -> Result<BlobConfig, Error> {
    match inner_blobstore_id {
        None => Ok(blob_config),
        Some(inner_blobstore_id) => match blob_config {
            BlobConfig::Multiplexed { blobstores, .. } => {
                let seeked_id = BlobstoreId::new(inner_blobstore_id);
                blobstores
                    .into_iter()
                    .find_map(|(blobstore_id, blobstore)| {
                        if blobstore_id == seeked_id {
                            Some(blobstore)
                        } else {
                            None
                        }
                    })
                    .ok_or(format_err!(
                        "could not find a blobstore with id {}",
                        inner_blobstore_id
                    ))
            }
            _ => Err(format_err!(
                "inner-blobstore-id supplied but blobstore is not multiplexed"
            )),
        },
    }
}

pub fn open_blobstore(
    fb: FacebookInit,
    mysql_options: MysqlOptions,
    storage_config: StorageConfig,
    inner_blobstore_id: Option<u64>,
    // TODO(ahornby) take multiple prefix for when scrubbing multiple repos
    prefix: Option<String>,
    readonly_storage: ReadOnlyStorage,
    scrub_action: Option<ScrubAction>,
    scuba_builder: ScubaSampleBuilder,
    walk_stats_key: &'static str,
    repo_stats_key: String,
    blobstore_options: BlobstoreOptions,
    logger: Logger,
) -> BoxFuture<(BoxFuture<Arc<dyn Blobstore>, Error>, SqlFactory), Error> {
    // Allow open of just one inner store
    let mut blobconfig =
        try_boxfuture!(get_blobconfig(storage_config.blobstore, inner_blobstore_id));

    let scrub_handler = scrub_action.map(|scrub_action| {
        blobconfig.set_scrubbed(scrub_action);
        Arc::new(StatsScrubHandler::new(
            false,
            scuba_builder.clone(),
            walk_stats_key,
            repo_stats_key.clone(),
        )) as Arc<dyn ScrubHandler>
    });

    let datasources = make_sql_factory(
        fb,
        storage_config.dbconfig,
        mysql_options,
        readonly_storage,
        logger,
    )
    .and_then(move |sql_factory| {
        let blobstore = match (scrub_handler, blobconfig) {
            (
                Some(scrub_handler),
                BlobConfig::Scrub {
                    scuba_table,
                    blobstores,
                    scrub_action,
                },
            ) => {
                // Make sure the repair stats are set to zero for each store.
                // Without this the new stats only show up when a repair is needed (i.e. as they get incremented),
                // which makes them harder to monitor on (no datapoints rather than a zero datapoint at start).
                for s in &[STATS::scrub_repaired, STATS::scrub_repair_required] {
                    for (id, _config) in &blobstores {
                        s.add_value(0, (walk_stats_key, id.to_string(), repo_stats_key.clone()));
                    }
                }

                make_blobstore_multiplexed(
                    fb,
                    &scuba_table,
                    &blobstores,
                    Some(&sql_factory),
                    mysql_options,
                    readonly_storage,
                    Some((scrub_handler, scrub_action)),
                    blobstore_options,
                )
            }
            (
                None,
                BlobConfig::Multiplexed {
                    scuba_table,
                    blobstores,
                },
            ) => make_blobstore_multiplexed(
                fb,
                &scuba_table,
                &blobstores,
                Some(&sql_factory),
                mysql_options,
                readonly_storage,
                None,
                blobstore_options,
            ),
            (None, blobconfig) => make_blobstore(
                fb,
                &blobconfig,
                &sql_factory,
                mysql_options,
                readonly_storage,
                blobstore_options,
            ),
            (Some(_), _) => {
                future::err(format_err!("Scrub action passed for non-scrubbable store")).boxify()
            }
        };
        future::ok((blobstore, sql_factory))
    });

    datasources
        .map(move |(storage, sql_factory)| {
            // Only need to prefix at this level if not using via blob repo, e.g. GC
            let maybe_prefixed = match prefix {
                Some(prefix) => storage
                    .map(|s| {
                        Arc::new(PrefixBlobstore::new(s, InlinableString::from(prefix)))
                            as Arc<dyn Blobstore>
                    })
                    .left_future(),
                None => storage.right_future(),
            };

            // Redaction would go here if needed
            (maybe_prefixed.boxify(), sql_factory)
        })
        .boxify()
}
