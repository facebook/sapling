/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use obc_lib::obc_client::OBCClient;
use obc_lib::obc_client::OBCClientOptionsBuilder;
use obc_lib::AggValue;
use obc_lib::OBCBumper as _;
use once_cell::sync::OnceCell;

static OBC_CLIENT: OnceCell<Arc<OBCClient>> = OnceCell::new();

pub(crate) type Counter = stats_traits::stat_types::BoxSingletonCounter;

pub(crate) fn new_counter(name: &'static str) -> Counter {
    stats::create_singleton_counter(name.to_string())
}

pub fn initialize_obc_client() -> anyhow::Result<()> {
    if !fbinit::was_performed() {
        return Err(anyhow::anyhow!(
            "Failed to initilize obc client for logging to ods"
        ));
    }

    if OBC_CLIENT.get().is_some() {
        return Ok(());
    }

    let opts = OBCClientOptionsBuilder::default()
        .ods_category("eden".to_string())
        .append_agg_type_suffix(false)
        .build()
        .map_err(anyhow::Error::msg)?;

    OBC_CLIENT
        .set(Arc::new(OBCClient::new(fbinit::expect_init(), opts)?))
        .map_err(|_| anyhow::anyhow!("Failed to initilize obc client for logging to ods"))
}

pub(crate) fn increment(counter: &Counter, name: &str, value: i64) {
    if !fbinit::was_performed() {
        return;
    }

    if let Some(client) = OBC_CLIENT.get() {
        let obc_entity = util::sys::hostname().to_string();
        let _ = client.bump_entity_key_agg(&obc_entity, name, AggValue::Sum(value as f64), None);
    }

    counter.increment_value(fbinit::expect_init(), value);
}
