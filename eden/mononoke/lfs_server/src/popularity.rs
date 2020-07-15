/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use fbinit::FacebookInit;
use mononoke_types::hash::Sha256;
use stats::prelude::*;
use std::convert::TryInto;
use std::time::Duration;
use time_window_counter::{BoxGlobalTimeWindowCounter, GlobalTimeWindowCounterBuilder};
use tokio::time::{self};

use crate::lfs_server_context::RepositoryRequestContext;

define_stats! {
    prefix = "mononoke.lfs.popularity";
    success: timeseries(Rate, Sum),
    error: timeseries(Rate, Sum),
    timeout: timeseries(Rate, Sum),
}

const OBJECT_POPULARITY_WINDOW: Duration = Duration::from_secs(20);
const OBJECT_POPULARITY_TIMEOUT: Duration = Duration::from_millis(10);

pub trait PopularityBuilder {
    fn build(
        &self,
        fb: FacebookInit,
        category: impl AsRef<str>,
        key: impl AsRef<str>,
        min_time_window: u32,
        max_time_window: u32,
    ) -> BoxGlobalTimeWindowCounter;
}

impl PopularityBuilder for GlobalTimeWindowCounterBuilder {
    fn build(
        &self,
        fb: FacebookInit,
        category: impl AsRef<str>,
        key: impl AsRef<str>,
        min_time_window: u32,
        max_time_window: u32,
    ) -> BoxGlobalTimeWindowCounter {
        GlobalTimeWindowCounterBuilder::build(fb, category, key, min_time_window, max_time_window)
    }
}

async fn increment_and_fetch_object_popularity<B: PopularityBuilder>(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
    builder: B,
) -> Result<Option<u64>, Error> {
    let category = match ctx.config.object_popularity_category() {
        Some(category) => category,
        None => {
            return Ok(None);
        }
    };

    let key = format!("{}/{}", ctx.repo.name(), oid);
    let window = OBJECT_POPULARITY_WINDOW.as_secs() as u32;

    let ctr = builder.build(ctx.ctx.fb, category, &key, window, window);

    ctr.bump(1.0);
    let v = ctr.get(window).await? as i64;
    let v = v.try_into()?;
    Ok(Some(v))
}

pub async fn allow_consistent_routing<B: PopularityBuilder>(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
    builder: B,
) -> bool {
    let popularity = time::timeout(
        OBJECT_POPULARITY_TIMEOUT,
        increment_and_fetch_object_popularity(ctx, oid, builder),
    )
    .await;

    // NOTE: We check the threshold after incrementing, since we want hosts to contribute to
    // counting even if the threshold isn't set (if we don't want them to count, then we don't set
    // object_popularity_category).
    let threshold = match ctx.config.object_popularity_threshold() {
        Some(threshold) => threshold,
        None => return true,
    };

    match popularity {
        Ok(Ok(Some(popularity))) => {
            STATS::success.add_value(1);
            return popularity <= threshold;
        }
        Ok(Ok(None)) => {
            // Not enabled.
        }
        Ok(Err(_)) => {
            // Errored
            STATS::error.add_value(1);
        }
        Err(..) => {
            // Timed out
            STATS::timeout.add_value(1);
        }
    };

    // Default to allowed
    true
}

#[cfg(test)]
mod test {
    use super::*;

    use async_trait::async_trait;
    use futures::future;
    use mononoke_types_mocks::hash::ONES_SHA256;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };
    use time_window_counter::GlobalTimeWindowCounter;

    use crate::config::ServerConfig;

    #[derive(Clone, Default)]
    struct DummyCounter(Arc<AtomicU64>);

    impl PopularityBuilder for DummyCounter {
        fn build(
            &self,
            _: FacebookInit,
            _: impl AsRef<str>,
            _: impl AsRef<str>,
            _: u32,
            _: u32,
        ) -> BoxGlobalTimeWindowCounter {
            Box::new(self.clone())
        }
    }

    #[async_trait]
    impl GlobalTimeWindowCounter for DummyCounter {
        async fn get(&self, _time_window: u32) -> Result<f64, Error> {
            Ok(self.0.load(Ordering::Relaxed) as f64)
        }

        fn bump(&self, value: f64) {
            self.0.fetch_add(value as u64, Ordering::Relaxed);
        }
    }

    #[derive(Copy, Clone)]
    struct TimeoutCounter;

    impl PopularityBuilder for TimeoutCounter {
        fn build(
            &self,
            _: FacebookInit,
            _: impl AsRef<str>,
            _: impl AsRef<str>,
            _: u32,
            _: u32,
        ) -> BoxGlobalTimeWindowCounter {
            Box::new(*self)
        }
    }

    #[async_trait]
    impl GlobalTimeWindowCounter for TimeoutCounter {
        async fn get(&self, _time_window: u32) -> Result<f64, Error> {
            future::pending().await
        }

        fn bump(&self, _: f64) {
            // noop
        }
    }

    #[derive(Copy, Clone)]
    struct ErrorCounter;

    impl PopularityBuilder for ErrorCounter {
        fn build(
            &self,
            _: FacebookInit,
            _: impl AsRef<str>,
            _: impl AsRef<str>,
            _: u32,
            _: u32,
        ) -> BoxGlobalTimeWindowCounter {
            Box::new(*self)
        }
    }

    #[async_trait]
    impl GlobalTimeWindowCounter for ErrorCounter {
        async fn get(&self, _time_window: u32) -> Result<f64, Error> {
            Err(Error::msg("error counter"))
        }

        fn bump(&self, _: f64) {
            // noop
        }
    }

    #[fbinit::test]
    async fn test_disabled(fb: FacebookInit) -> Result<(), Error> {
        let ctx = RepositoryRequestContext::test_builder(fb)?.build()?;
        let ctr = DummyCounter::default();

        assert_eq!(allow_consistent_routing(&ctx, ONES_SHA256, ctr).await, true);

        Ok(())
    }

    #[fbinit::test]
    async fn test_recording_only(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        config.raw_server_config.object_popularity_category = Some("foo".to_string());

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .config(config)
            .build()?;
        let ctr = DummyCounter::default();

        assert_eq!(
            allow_consistent_routing(&ctx, ONES_SHA256, ctr.clone()).await,
            true
        );
        assert!(ctr.0.load(Ordering::Relaxed) > 0);

        Ok(())
    }

    #[fbinit::test]
    async fn test_popularity(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        config.raw_server_config.object_popularity_category = Some("foo".to_string());
        config.raw_server_config.object_popularity_threshold = Some(10);

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .config(config)
            .build()?;
        let ctr = DummyCounter::default();

        assert_eq!(
            allow_consistent_routing(&ctx, ONES_SHA256, ctr.clone()).await,
            true
        );

        ctr.0.fetch_add(10, Ordering::Relaxed);

        assert_eq!(
            allow_consistent_routing(&ctx, ONES_SHA256, ctr.clone()).await,
            false
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_timeout(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        config.raw_server_config.object_popularity_category = Some("foo".to_string());
        config.raw_server_config.object_popularity_threshold = Some(10);

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .config(config)
            .build()?;

        assert_eq!(
            allow_consistent_routing(&ctx, ONES_SHA256, TimeoutCounter).await,
            true
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_error(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        config.raw_server_config.object_popularity_category = Some("foo".to_string());
        config.raw_server_config.object_popularity_threshold = Some(10);

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .config(config)
            .build()?;

        assert_eq!(
            allow_consistent_routing(&ctx, ONES_SHA256, ErrorCounter).await,
            true
        );

        Ok(())
    }
}
