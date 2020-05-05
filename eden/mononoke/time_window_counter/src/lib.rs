/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
mod facebook;

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub type ArcGlobalTimeWindowCounter = Arc<dyn GlobalTimeWindowCounter + Send + Sync + 'static>;
pub type BoxGlobalTimeWindowCounter = Box<dyn GlobalTimeWindowCounter + Send + Sync + 'static>;

#[async_trait]
pub trait GlobalTimeWindowCounter {
    async fn get(&self, time_window: u32) -> Result<f64>;

    fn bump(&self, value: f64);
}

pub struct GlobalTimeWindowCounterBuilder {}

#[cfg(not(fbcode_build))]
mod r#impl {
    use super::*;

    use fbinit::FacebookInit;

    struct AlwaysZeroCounter {}

    #[async_trait]
    impl GlobalTimeWindowCounter for AlwaysZeroCounter {
        async fn get(&self, _time_window: u32) -> Result<f64> {
            Ok(0.0)
        }

        fn bump(&self, _value: f64) {}
    }

    impl GlobalTimeWindowCounterBuilder {
        pub fn build(
            _fb: FacebookInit,
            _category: impl AsRef<str>,
            _key: impl AsRef<str>,
            _min_time_window: u32,
            _max_time_window: u32,
        ) -> BoxGlobalTimeWindowCounter {
            Box::new(AlwaysZeroCounter {})
        }
    }
}
