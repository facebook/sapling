/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::validate::{CHECK_FAIL, CHECK_TYPE, NODE_KEY, REPO};

use anyhow::{format_err, Error};
use blobstore::Blobstore;
use blobstore_factory::{
    make_blobstore, make_blobstore_multiplexed, BlobstoreOptions, ReadOnlyStorage,
};
use context::CoreContext;
use fbinit::FacebookInit;
use futures_preview::compat::Future01CompatExt;
use inlinable_string::InlinableString;
use metaconfig_types::{BlobConfig, BlobstoreId, ScrubAction};
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
            .add("session", ctx.session().session_id().to_string())
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

pub async fn open_blobstore(
    fb: FacebookInit,
    mysql_options: MysqlOptions,
    blob_config: BlobConfig,
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
) -> Result<Arc<dyn Blobstore>, Error> {
    let mut blobconfig = get_blobconfig(blob_config, inner_blobstore_id)?;
    let scrub_handler = scrub_action.map(|scrub_action| {
        blobconfig.set_scrubbed(scrub_action);
        Arc::new(StatsScrubHandler::new(
            false,
            scuba_builder.clone(),
            walk_stats_key,
            repo_stats_key.clone(),
        )) as Arc<dyn ScrubHandler>
    });

    let blobstore = match (scrub_handler, blobconfig) {
        (
            Some(scrub_handler),
            BlobConfig::Scrub {
                multiplex_id,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                scrub_action,
                queue_db,
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
                multiplex_id,
                queue_db,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                mysql_options,
                readonly_storage,
                Some((scrub_handler, scrub_action)),
                blobstore_options,
                logger,
            )
            .compat()
            .await?
        }
        (
            None,
            BlobConfig::Multiplexed {
                multiplex_id,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                queue_db,
            },
        ) => {
            make_blobstore_multiplexed(
                fb,
                multiplex_id,
                queue_db,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                mysql_options,
                readonly_storage,
                None,
                blobstore_options,
                logger,
            )
            .compat()
            .await?
        }
        (None, blobconfig) => {
            make_blobstore(
                fb,
                blobconfig,
                mysql_options,
                readonly_storage,
                blobstore_options,
                logger,
            )
            .compat()
            .await?
        }
        (Some(_), _) => {
            return Err(format_err!("Scrub action passed for non-scrubbable store"));
        }
    };

    if let Some(prefix) = prefix {
        return Ok(Arc::new(PrefixBlobstore::new(
            blobstore,
            InlinableString::from(prefix),
        )));
    }

    Ok(blobstore)
}
