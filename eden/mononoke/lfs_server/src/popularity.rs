/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU16;
use std::time::Duration;

use anyhow::Error;
use fbinit::FacebookInit;
use slog::error;
use stats::prelude::*;
use time_window_counter::BoxGlobalTimeWindowCounter;
use time_window_counter::GlobalTimeWindowCounterBuilder;
use tokio::time;

use crate::batch::InternalObject;
use crate::config::ConsistentRoutingRingMode;
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

/// None -> no consistent routing - routing randomly to all tasks
/// Some(n) -> consistently route to n tasks
///
/// By default, route all blobs consistently to one task - Some(1)
pub async fn consistent_routing<B: PopularityBuilder>(
    ctx: &RepositoryRequestContext,
    obj: InternalObject,
    builder: B,
) -> Option<NonZeroU16> {
    let config = match ctx.config.object_popularity() {
        Some(r) => r,
        None => return Some(NonZeroU16::new(1).unwrap()),
    };

    let popularity = time::timeout(
        OBJECT_POPULARITY_TIMEOUT,
        increment_and_fetch_object_popularity(ctx, obj, config, builder),
    )
    .await;

    match popularity {
        Ok(Ok(popularity)) => {
            STATS::success.add_value(1);
            // The bigger the popularity, the bigger tasks_per_content.
            // Thresholds are sorted in increasing order.
            // [pop_thresh = 0,    tasks_per_content = 1]
            // [pop_thresh = 100,  tasks_per_content = 10]
            // [pop_thresh = 1000, tasks_per_content = All]
            for ring in config.thresholds.iter().rev() {
                if popularity >= ring.threshold {
                    match ring.mode {
                        ConsistentRoutingRingMode::Num { tasks_per_content } => {
                            return Some(tasks_per_content);
                        }
                        ConsistentRoutingRingMode::All => return None,
                    }
                }
            }
            // Shouldn't be reached as we require to have a ring with popularity
            // threshold 0. Let's default to 1 anyway.
        }
        Ok(Err(e)) => {
            // Errored
            error!(ctx.logger(), "popularity metric failed: {:?}", e);
            STATS::error.add_value(1);
        }
        Err(..) => {
            // Timed out
            STATS::timeout.add_value(1);
        }
    };

    // Default to one task per blob
    Some(NonZeroU16::new(1).unwrap())
}

#[cfg(test)]
mod test {
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use async_trait::async_trait;
    use futures::future;
    use mononoke_types_mocks::contentid::ONES_CTID;
    use mononoke_types_mocks::hash::ONES_SHA256;
    use time_window_counter::GlobalTimeWindowCounter;

    use super::*;
    use crate::config::ConsistentRoutingRing;
    use crate::config::ConsistentRoutingRingMode;
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
        let ctx = RepositoryRequestContext::test_builder(fb).await?.build()?;
        let ctr = DummyCounter::default();

        assert!(consistent_routing(&ctx, dummy(None), ctr).await.is_some());

        Ok(())
    }

    #[fbinit::test]
    async fn test_popularity(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        *config.object_popularity_mut() = Some(ObjectPopularity {
            category: "foo".into(),
            window: 100,
            thresholds: vec![
                ConsistentRoutingRing {
                    threshold: 0,
                    mode: ConsistentRoutingRingMode::Num {
                        tasks_per_content: std::num::NonZeroU16::new(1).unwrap(),
                    },
                },
                ConsistentRoutingRing {
                    threshold: 10,
                    mode: ConsistentRoutingRingMode::All,
                },
            ],
        });

        let ctx = RepositoryRequestContext::test_builder(fb)
            .await?
            .config(config)
            .build()?;
        let ctr = DummyCounter::default();

        assert!(
            consistent_routing(&ctx, dummy(4), ctr.clone())
                .await
                .is_some()
        );

        assert!(
            consistent_routing(&ctx, dummy(4), ctr.clone())
                .await
                .is_some()
        );

        assert!(
            consistent_routing(&ctx, dummy(4), ctr.clone())
                .await
                .is_none()
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_popularity_rings(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        *config.object_popularity_mut() = Some(ObjectPopularity {
            category: "foo".into(),
            window: 100,
            thresholds: vec![
                ConsistentRoutingRing {
                    threshold: 0,
                    mode: ConsistentRoutingRingMode::Num {
                        tasks_per_content: std::num::NonZeroU16::new(1).unwrap(),
                    },
                },
                ConsistentRoutingRing {
                    threshold: 10,
                    mode: ConsistentRoutingRingMode::Num {
                        tasks_per_content: std::num::NonZeroU16::new(10).unwrap(),
                    },
                },
                ConsistentRoutingRing {
                    threshold: 15,
                    mode: ConsistentRoutingRingMode::All,
                },
            ],
        });

        let ctx = RepositoryRequestContext::test_builder(fb)
            .await?
            .config(config)
            .build()?;
        let ctr = DummyCounter::default();

        assert_eq!(
            consistent_routing(&ctx, dummy(4), ctr.clone()).await,
            Some(std::num::NonZeroU16::new(1).unwrap())
        );

        assert_eq!(
            consistent_routing(&ctx, dummy(4), ctr.clone()).await,
            Some(std::num::NonZeroU16::new(1).unwrap())
        );

        assert_eq!(
            consistent_routing(&ctx, dummy(4), ctr.clone()).await,
            Some(std::num::NonZeroU16::new(10).unwrap())
        );

        assert!(
            consistent_routing(&ctx, dummy(4), ctr.clone())
                .await
                .is_none()
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_timeout(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        *config.object_popularity_mut() = Some(ObjectPopularity {
            category: "foo".into(),
            window: 100,
            thresholds: vec![
                ConsistentRoutingRing {
                    threshold: 0,
                    mode: ConsistentRoutingRingMode::Num {
                        tasks_per_content: std::num::NonZeroU16::new(1).unwrap(),
                    },
                },
                ConsistentRoutingRing {
                    threshold: 10,
                    mode: ConsistentRoutingRingMode::All,
                },
            ],
        });

        let ctx = RepositoryRequestContext::test_builder(fb)
            .await?
            .config(config)
            .build()?;

        assert!(
            consistent_routing(&ctx, dummy(None), TimeoutCounter)
                .await
                .is_some()
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_error(fb: FacebookInit) -> Result<(), Error> {
        let mut config = ServerConfig::default();
        *config.object_popularity_mut() = Some(ObjectPopularity {
            category: "foo".into(),
            window: 100,
            thresholds: vec![
                ConsistentRoutingRing {
                    threshold: 0,
                    mode: ConsistentRoutingRingMode::Num {
                        tasks_per_content: std::num::NonZeroU16::new(1).unwrap(),
                    },
                },
                ConsistentRoutingRing {
                    threshold: 10,
                    mode: ConsistentRoutingRingMode::All,
                },
            ],
        });

        let ctx = RepositoryRequestContext::test_builder(fb)
            .await?
            .config(config)
            .build()?;

        assert!(
            consistent_routing(&ctx, dummy(None), ErrorCounter)
                .await
                .is_some()
        );

        Ok(())
    }
}
