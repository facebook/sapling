/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use obc_lib::AggValue;
use obc_lib::OBCBumper as _;
use obc_lib::obc_client::OBCClient;
use obc_lib::obc_client::OBCClientOptions;
use once_cell::sync::OnceCell;
use sysutil::hostname;

struct OBCClientWrapper {
    client: Arc<OBCClient>,
    entity_keys: Vec<String>,
}

impl OBCClientWrapper {
    fn new(client: Arc<OBCClient>) -> Self {
        let hostname = hostname().to_string();
        let remote_execution_worker = std::env::var("REMOTE_EXECUTION_WORKER").ok();
        let entity_keys = if let Some(worker) = remote_execution_worker {
            vec![hostname.clone(), format!("{}:{}", hostname, worker)]
        } else {
            vec![hostname]
        };
        OBCClientWrapper {
            client,
            entity_keys,
        }
    }

    fn bump_entity_key_agg(&self, name: &str, value: i64) {
        for entity_key in &self.entity_keys {
            let _ = self.client.bump_entity_key_agg(
                entity_key,
                name,
                AggValue::Sum(value as f64),
                None,
            );
        }
    }
}

static OBC_CLIENT: OnceCell<OBCClientWrapper> = OnceCell::new();

pub(crate) type Counter = stats_traits::stat_types::BoxSingletonCounter;

pub(crate) fn new_counter(name: &'static str) -> Counter {
    stats::create_singleton_counter(name.to_string())
}

pub fn initialize_obc_client() -> anyhow::Result<()> {
    if !fbinit::was_performed() {
        return Err(anyhow::anyhow!(
            "Failed to initialize obc client for logging to ods"
        ));
    }
    OBC_CLIENT.get_or_try_init(|| -> anyhow::Result<OBCClientWrapper> {
        let opts = OBCClientOptions::builder()
            .ods_category("eden")
            .append_agg_type_suffix(false)
            .build();
        Ok(OBCClientWrapper::new(Arc::new(OBCClient::new(
            fbinit::expect_init(),
            opts,
        )?)))
    })?;
    Ok(())
}

pub(crate) fn increment(counter: &Counter, name: &str, value: i64) {
    if !fbinit::was_performed() {
        return;
    }
    if let Some(client) = OBC_CLIENT.get() {
        client.bump_entity_key_agg(name, value);
    }
    counter.increment_value(fbinit::expect_init(), value);
}
