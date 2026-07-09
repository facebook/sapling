/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::Result;
use metrics::Sink;
use obc_lib::AggValue;
use obc_lib::OBCBumper as _;
use obc_lib::obc_client::OBCClient;
use obc_lib::obc_client::OBCClientOptions;
use parking_lot::Mutex;
use parking_lot::RwLock;
use stats_traits::stat_types::BoxSingletonCounter;
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
            if let Err(err) =
                self.client
                    .bump_entity_key_agg(entity_key, name, AggValue::Sum(value as f64), None)
            {
                tracing::warn!(?err, entity_key, metric = name, "failed to bump OBC metric");
            }
        }
    }
}

struct FbSink {
    counters: RwLock<HashMap<&'static str, BoxSingletonCounter>>,
    obc_client: OnceLock<OBCClientWrapper>,
}

impl FbSink {
    fn new() -> Result<Self> {
        if !fbinit::was_performed() {
            return Err(anyhow::anyhow!(
                "failed to install fb metrics sink without fbinit"
            ));
        }

        Ok(Self {
            counters: RwLock::new(HashMap::new()),
            obc_client: OnceLock::new(),
        })
    }

    fn enable_obc(&self) -> Result<()> {
        if self.obc_client.get().is_some() {
            return Ok(());
        }

        let opts = OBCClientOptions::builder()
            .ods_category("eden")
            .append_agg_type_suffix(false)
            .build();
        let client = OBCClientWrapper::new(Arc::new(OBCClient::new(fbinit::expect_init(), opts)?));
        match self.obc_client.set(client) {
            // Another caller may win the race after the fast-path check above.
            Ok(()) | Err(_) => {}
        }
        Ok(())
    }
}

impl Sink for FbSink {
    fn increment(&self, name: &'static str, value: i64) {
        if let Some(client) = self.obc_client.get() {
            client.bump_entity_key_agg(name, value);
        }

        if let Some(counter) = self.counters.read().get(name) {
            counter.increment_value(fbinit::expect_init(), value);
            return;
        }

        let mut counters = self.counters.write();
        let counter = counters
            .entry(name)
            .or_insert_with(|| stats::create_singleton_counter(name.to_string()));
        counter.increment_value(fbinit::expect_init(), value);
    }
}

pub fn install(enable_obc: bool) -> Result<()> {
    static FB_SINK: Mutex<Option<Arc<FbSink>>> = Mutex::new(None);

    let sink = {
        let mut slot = FB_SINK.lock();
        if let Some(sink) = slot.as_ref() {
            sink.clone()
        } else {
            let sink = Arc::new(FbSink::new()?);
            metrics::install_sink(sink.clone())?;
            *slot = Some(sink.clone());
            sink
        }
    };

    if enable_obc {
        sink.enable_obc()?;
    }

    Ok(())
}
