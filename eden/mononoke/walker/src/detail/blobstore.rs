/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::detail::pack::CTIME;
use crate::detail::validate::CHECK_FAIL;
use crate::detail::validate::CHECK_TYPE;
use crate::detail::validate::ERROR_MSG;
use crate::detail::validate::NODE_KEY;
use crate::detail::validate::REPO;

use anyhow::anyhow;
use anyhow::Error;
use blobstore::BlobstoreMetadata;
use context::CoreContext;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use mononoke_types::repo::REPO_PREFIX_REGEX;
use mononoke_types::RepositoryId;
use multiplexedblob::LoggingScrubHandler;
use multiplexedblob::ScrubHandler;
use scuba::value::NullScubaValue;
use scuba::value::ScubaValue;
use scuba_ext::MononokeScubaSampleBuilder;
use stats::prelude::*;
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

define_stats! {
    prefix = "mononoke.walker";
    scrub_repaired: dynamic_timeseries("{}.blobstore.{}.{}.repaired", (subcommand: &'static str, blobstore_id: String, repo: String); Rate, Sum),
    scrub_repair_required: dynamic_timeseries("{}.blobstore.{}.{}.repair_required", (subcommand: &'static str, blobstore_id: String, repo: String); Rate, Sum),
}

pub const BLOBSTORE_ID: &str = "blobstore_id";

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

impl fmt::Debug for StatsScrubHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatsScrubHandler")
            .field("subcommand_stats_key", &self.subcommand_stats_key)
            .field("inner", &self.inner)
            .finish()
    }
}

pub fn get_repo_id_from_key(key: &str) -> Result<Option<RepositoryId>, Error> {
    REPO_PREFIX_REGEX
        .captures(key)
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
            .add(CTIME, ctime)
            .log();
    }
}

pub fn replace_blobconfig(
    blob_config: &mut BlobConfig,
    inner_blobstore_id: Option<u64>,
    repo_name: &str,
    walk_stats_key: &'static str,
    is_scrubbing: bool,
) -> Result<(), Error> {
    match blob_config {
        BlobConfig::Multiplexed { ref blobstores, .. } => {
            if is_scrubbing {
                // Make sure the repair stats are set to zero for each store.
                // Without this the new stats only show up when a repair is
                // needed (i.e. as they get incremented), which makes them
                // harder to monitor on (no datapoints rather than a zero
                // datapoint at start).
                for s in &[STATS::scrub_repaired, STATS::scrub_repair_required] {
                    for (id, _ty, _config) in blobstores {
                        s.add_value(0, (walk_stats_key, id.to_string(), repo_name.to_string()));
                    }
                }
            }
            if let Some(inner_blobstore_id) = inner_blobstore_id {
                let sought_id = BlobstoreId::new(inner_blobstore_id);
                let inner_blob_config = blobstores
                    .iter()
                    .find_map(|(blobstore_id, _, blobstore)| {
                        if blobstore_id == &sought_id {
                            Some(blobstore.clone())
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| {
                        anyhow!("could not find a blobstore with id {}", inner_blobstore_id)
                    })?;
                *blob_config = inner_blob_config;
            }
        }
        _ => {
            if inner_blobstore_id.is_some() {
                return Err(anyhow!(
                    "inner-blobstore-id supplied but blobstore is not multiplexed"
                ));
            }
        }
    }
    Ok(())
}
