/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::validate::{CHECK_FAIL, CHECK_TYPE, ERROR_MSG, NODE_KEY, REPO};

use anyhow::{format_err, Error};
use blobstore::{Blobstore, BlobstoreMetadata};
use blobstore_factory::{
    make_blobstore_multiplexed, make_blobstore_put_ops, BlobstoreOptions, ReadOnlyStorage,
};
use cached_config::ConfigStore;
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::{BlobConfig, BlobstoreId, ScrubAction};
use mononoke_types::{repo::REPO_PREFIX_REGEX, RepositoryId};
use multiplexedblob::{LoggingScrubHandler, ScrubHandler};
use samplingblob::{SamplingBlobstore, SamplingHandler};
use scuba::value::{NullScubaValue, ScubaValue};
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use stats::prelude::*;
use std::{collections::HashMap, convert::From, str::FromStr, sync::Arc};

define_stats! {
    prefix = "mononoke.walker";
    scrub_repaired: dynamic_timeseries("{}.blobstore.{}.{}.repaired", (subcommand: &'static str, blobstore_id: String, repo: String); Rate, Sum),
    scrub_repair_required: dynamic_timeseries("{}.blobstore.{}.{}.repair_required", (subcommand: &'static str, blobstore_id: String, repo: String); Rate, Sum),
}

pub const BLOBSTORE_ID: &'static str = "blobstore_id";

pub struct StatsScrubHandler {
    scuba: MononokeScubaSampleBuilder,
    subcommand_stats_key: &'static str,
    repo_id_to_name: HashMap<RepositoryId, String>,
    inner: LoggingScrubHandler,
}

impl StatsScrubHandler {
    pub fn new(
        quiet: bool,
        scuba: MononokeScubaSampleBuilder,
        subcommand_stats_key: &'static str,
        repo_id_to_name: HashMap<RepositoryId, String>,
    ) -> Self {
        Self {
            scuba,
            subcommand_stats_key,
            inner: LoggingScrubHandler::new(quiet),
            repo_id_to_name,
        }
    }
}

pub fn get_repo_id_from_key(key: &str) -> Result<Option<RepositoryId>, Error> {
    REPO_PREFIX_REGEX
        .captures(&key)
        .and_then(|m| m.get(1).map(|m| RepositoryId::from_str(m.as_str())))
        .transpose()
}

impl ScrubHandler for StatsScrubHandler {
    fn on_repair(
        &self,
        ctx: &CoreContext,
        blobstore_id: BlobstoreId,
        key: &str,
        is_repaired: bool,
        meta: &BlobstoreMetadata,
    ) {
        self.inner
            .on_repair(ctx, blobstore_id, key, is_repaired, meta);

        let ctime = match meta.ctime() {
            Some(ctime) => ScubaValue::from(ctime),
            None => ScubaValue::Null(NullScubaValue::Int),
        };

        let mut scuba = self.scuba.clone();

        let repo_id = get_repo_id_from_key(key);
        match repo_id {
            Ok(Some(repo_id)) => {
                if let Some(repo_name) = self.repo_id_to_name.get(&repo_id) {
                    scuba.add(REPO, repo_name.clone());
                    if is_repaired {
                        STATS::scrub_repaired.add_value(
                            1,
                            (
                                self.subcommand_stats_key,
                                blobstore_id.to_string(),
                                repo_name.clone(),
                            ),
                        );
                    } else {
                        STATS::scrub_repair_required.add_value(
                            1,
                            (
                                self.subcommand_stats_key,
                                blobstore_id.to_string(),
                                repo_name.clone(),
                            ),
                        );
                    }
                }
            }
            Ok(_) => {}
            Err(e) => {
                scuba.add(ERROR_MSG, format!("{:?}", e));
            }
        }

        scuba
            .add(BLOBSTORE_ID, blobstore_id)
            // TODO parse out NodeType from string key prefix if we can. Or better, make blobstore keys typed?
            .add(NODE_KEY, key)
            .add(CHECK_TYPE, "scrub_repair")
            .add(CHECK_FAIL, if is_repaired { 0 } else { 1 })
            .add("session", ctx.session().metadata().session_id().to_string())
            .add("ctime", ctime)
            .log();
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
                    .find_map(|(blobstore_id, _, blobstore)| {
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

pub async fn open_blobstore<'a>(
    fb: FacebookInit,
    mysql_options: &'a MysqlOptions,
    blob_config: BlobConfig,
    inner_blobstore_id: Option<u64>,
    readonly_storage: ReadOnlyStorage,
    scrub_action: Option<ScrubAction>,
    blobstore_sampler: Option<Arc<dyn SamplingHandler>>,
    scuba_builder: MononokeScubaSampleBuilder,
    walk_stats_key: &'static str,
    repo_id_to_name: HashMap<RepositoryId, String>,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
) -> Result<Arc<dyn Blobstore>, Error> {
    let mut blobconfig = get_blobconfig(blob_config, inner_blobstore_id)?;
    let scrub_handler = scrub_action.map(|scrub_action| {
        blobconfig.set_scrubbed(scrub_action);
        Arc::new(StatsScrubHandler::new(
            false,
            scuba_builder.clone(),
            walk_stats_key,
            repo_id_to_name.clone(),
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
                minimum_successful_writes,
                scrub_action,
                queue_db,
            },
        ) => {
            // Make sure the repair stats are set to zero for each store.
            // Without this the new stats only show up when a repair is needed (i.e. as they get incremented),
            // which makes them harder to monitor on (no datapoints rather than a zero datapoint at start).
            for name in repo_id_to_name.values() {
                for s in &[STATS::scrub_repaired, STATS::scrub_repair_required] {
                    for (id, _ty, _config) in &blobstores {
                        s.add_value(0, (walk_stats_key, id.to_string(), name.clone()));
                    }
                }
            }

            make_blobstore_multiplexed(
                fb,
                multiplex_id,
                queue_db,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                minimum_successful_writes,
                Some((scrub_handler, scrub_action)),
                mysql_options,
                readonly_storage,
                blobstore_options,
                logger,
                config_store,
            )
            .await?
        }
        (
            None,
            BlobConfig::Multiplexed {
                multiplex_id,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                minimum_successful_writes,
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
                minimum_successful_writes,
                None,
                mysql_options,
                readonly_storage,
                blobstore_options,
                logger,
                config_store,
            )
            .await?
        }
        (None, blobconfig) => {
            make_blobstore_put_ops(
                fb,
                blobconfig,
                mysql_options,
                readonly_storage,
                blobstore_options,
                logger,
                config_store,
            )
            .await?
        }
        (Some(_), _) => {
            return Err(format_err!("Scrub action passed for non-scrubbable store"));
        }
    };

    let blobstore = match blobstore_sampler {
        Some(blobstore_sampler) => Arc::new(SamplingBlobstore::new(blobstore, blobstore_sampler))
            as Arc<dyn blobstore::Blobstore>,
        None => Arc::new(blobstore) as Arc<dyn blobstore::Blobstore>,
    };

    Ok(blobstore)
}
