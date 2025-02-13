/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use fbinit::FacebookInit;
use tokio::time::Duration;

use crate::CounterManager;

#[derive(Clone)]
pub struct OdsCounterManager {}

impl OdsCounterManager {
    pub fn new(_fb: FacebookInit) -> Self {
        Self {}
    }
}

#[async_trait]
impl CounterManager for OdsCounterManager {
    async fn add_counter(&mut self, _entity: String, _key: String) {}

    async fn run_periodic_fetch(&mut self, _interval_duration: Duration) {}

    async fn get_counter_value(&self, _entity: &str, _key: &str) -> Option<f64> {
        None
    }
}
