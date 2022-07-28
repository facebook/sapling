/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use fbinit::FacebookInit;
use stats::prelude::*;
use std::time::Duration;
use time_window_counter::BoxGlobalTimeWindowCounter;
use time_window_counter::GlobalTimeWindowCounterBuilder;
use tokio::time;

use crate::batch::InternalObject;
use crate::config::ObjectPopularity;
use crate::lfs_server_context::RepositoryRequestContext;

define_stats! {
    prefix = "mononoke.lfs.popularity";
    success: timeseries(Rate, Sum),
    error: timeseries(Rate, Sum),
    timeout: timeseries(Rate, Sum),
}

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
    obj: InternalObject,
    config: &ObjectPopularity,
    builder: B,
) -> Result<u64, Error> {
    let key = format!("{}/{}", &ctx.repo.name, obj.id());
    let ctr = builder.build(
        ctx.ctx.fb,
        &config.category,
        &key,
        config.window,
        config.window,
    );

    ctr.bump(obj.download_size() as f64);
    let v = ctr.get(config.window).await? as i64;
    let v = v.try_into()?;
    Ok(v)
}

pub async fn allow_consistent_routing<B: PopularityBuilder>(
    ctx: &RepositoryRequestContext,
    obj: InternalObject,
    builder: B,
) -> bool {
    let config = match ctx.config.object_popularity() {
        Some(r) => r,
        None => return true,
    };

    let popularity = time::timeout(
        OBJECT_POPULARITY_TIMEOUT,
        increment_and_fetch_object_popularity(ctx, obj, config, builder),
    )
    .await;

    match popularity {
        Ok(Ok(popularity)) => {
            STATS::success.add_value(1);
            return popularity <= config.threshold;
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
    use mononoke_types_mocks::contentid::ONES_CTID;
    use mononoke_types_mocks::hash::ONES_SHA256;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use time_window_counter::GlobalTimeWindowCounter;

    use crate::config::ObjectPopularity;
    use crate::config::ServerConfig;

    fn dummy(size: impl Into<Option<u64>>) -> InternalObject {
        InternalObject::new(ONES_CTID, ONES_SHA256, size.into())
    }

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

        assert!(allow_consistent_routing(&ctx, dummy(None), ctr).await);

        Ok(())
    }

    #[fbinit::test]
    async fn test_popularity(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        *config.object_popularity_mut() = Some(ObjectPopularity {
            category: "foo".into(),
            window: 100,
            threshold: 10,
        });

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .config(config)
            .build()?;
        let ctr = DummyCounter::default();

        assert_eq!(
            allow_consistent_routing(&ctx, dummy(4), ctr.clone()).await,
            true
        );

        assert_eq!(
            allow_consistent_routing(&ctx, dummy(4), ctr.clone()).await,
            true
        );

        assert_eq!(
            allow_consistent_routing(&ctx, dummy(4), ctr.clone()).await,
            false
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_timeout(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        *config.object_popularity_mut() = Some(ObjectPopularity {
            category: "foo".into(),
            window: 100,
            threshold: 10,
        });

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .config(config)
            .build()?;

        assert_eq!(
            allow_consistent_routing(&ctx, dummy(None), TimeoutCounter).await,
            true
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_error(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        *config.object_popularity_mut() = Some(ObjectPopularity {
            category: "foo".into(),
            window: 100,
            threshold: 10,
        });

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .config(config)
            .build()?;

        assert_eq!(
            allow_consistent_routing(&ctx, dummy(None), ErrorCounter).await,
            true
        );

        Ok(())
    }
}
