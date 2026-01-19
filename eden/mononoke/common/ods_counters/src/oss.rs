/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::RwLock;

use async_trait::async_trait;
use fbinit::FacebookInit;
use tokio::time::Duration;

use crate::CounterManager;

#[derive(Clone)]
pub struct OdsCounterManager {}

impl OdsCounterManager {
    pub fn new(_fb: FacebookInit) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self {}))
    }
}

#[async_trait]
impl CounterManager for OdsCounterManager {
    fn add_counter(
        &mut self,
        _entity: String,
        _key: String,
        _reduce: Option<String>,
        _transform: Option<String>,
    ) {
    }

    fn get_counter_value(
        &self,
        _entity: &str,
        _key: &str,
        _reduce: Option<&str>,
        _transform: Option<&str>,
    ) -> Option<f64> {
        None
    }
}

pub async fn periodic_fetch_counter(
    _manager: Arc<RwLock<OdsCounterManager>>,
    _interval_duration: Duration,
) {
}
