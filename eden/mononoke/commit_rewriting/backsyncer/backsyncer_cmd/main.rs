/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod run;
mod sharding;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::Error;
use cmdlib::args;
use cmdlib::helpers;
use executor_lib::ShardedProcessExecutor;
use fbinit::FacebookInit;

use crate::run::run_backsyncer;
use crate::sharding::BacksyncProcess;
use crate::sharding::APP_NAME;
use crate::sharding::DEFAULT_SHARDED_SCOPE_NAME;
use crate::sharding::SM_CLEANUP_TIMEOUT_SECS;

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let process = BacksyncProcess::new(fb)?;
    match process.matches.value_of(cmdlib::args::SHARDED_SERVICE_NAME) {
        Some(service_name) => {
            // Don't fail if the scope name is missing, but use global. This allows us to be
            // backward compatible with the old tw setup.
            // TODO (Pierre): Once the tw jobs have been updated, we can be less lenient here.
            let scope_name = process
                .matches
                .value_of(cmdlib::args::SHARDED_SCOPE_NAME)
                .unwrap_or(DEFAULT_SHARDED_SCOPE_NAME);
            // The service name needs to be 'static to satisfy SM contract
            static SM_SERVICE_NAME: OnceLock<String> = OnceLock::new();
            static SM_SERVICE_SCOPE_NAME: OnceLock<String> = OnceLock::new();
            let logger = process.matches.logger().clone();
            let matches = Arc::clone(&process.matches);
            let mut executor = ShardedProcessExecutor::new(
                process.fb,
                process.matches.runtime().clone(),
                &logger,
                SM_SERVICE_NAME.get_or_init(|| service_name.to_string()),
                SM_SERVICE_SCOPE_NAME.get_or_init(|| scope_name.to_string()),
                SM_CLEANUP_TIMEOUT_SECS,
                Arc::new(process),
                true, // enable shard (repo) level healing
            )?;
            helpers::block_execute(
                executor.block_and_execute(&logger, Arc::new(AtomicBool::new(false))),
                fb,
                &std::env::var("TW_JOB_NAME").unwrap_or_else(|_| APP_NAME.to_string()),
                matches.logger(),
                &matches,
                cmdlib::monitoring::AliveService,
            )
        }
        None => {
            let logger = process.matches.logger().clone();
            let matches = process.matches.clone();
            let config_store = matches.config_store();
            let source_repo_id =
                args::not_shardmanager_compatible::get_source_repo_id(config_store, &matches)?;
            let target_repo_id =
                args::not_shardmanager_compatible::get_target_repo_id(config_store, &matches)?;
            let (source_repo_name, _) =
                args::get_config_by_repoid(config_store, &matches, source_repo_id)?;
            let (target_repo_name, _) =
                args::get_config_by_repoid(config_store, &matches, target_repo_id)?;
            let fut = run_backsyncer(
                fb,
                matches.clone(),
                source_repo_name,
                target_repo_name,
                Arc::new(AtomicBool::new(false)),
            );
            helpers::block_execute(
                fut,
                fb,
                APP_NAME,
                &logger,
                &matches,
                cmdlib::monitoring::AliveService,
            )
        }
    }
}
