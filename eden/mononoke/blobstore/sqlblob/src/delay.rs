/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::min;
use std::num::NonZeroUsize;
use std::time::Duration;
use std::time::Instant;

use futures::stream::StreamExt;
use rand::thread_rng;
use rand::Rng;
use stats::prelude::*;
use tokio::sync::watch;

// This can be tweaked later.
pub(crate) const MAX_LAG: Duration = Duration::from_secs(5);

define_stats! {
    prefix = "mononoke.sqlblob.lag_delay";
    total_delay_ms: dynamic_timeseries("{}.total_delay_ms", (entity: String); Rate, Sum),
    raw_lag_ms: dynamic_timeseries("{}.raw_lag_ms", (entity: String); Rate, Sum),
}

#[derive(Clone)]
pub struct BlobDelay {
    lag_receivers: Vec<watch::Receiver<Duration>>,
    entity: Option<String>,
}

// Adds a small amount of random delay to desynchronise when waiting
async fn jitter_delay(raw_lag: Duration) {
    let delay =
        thread_rng().gen_range(Duration::from_secs(0)..min(Duration::from_secs(1), raw_lag));
    tokio::time::sleep(delay).await;
}

impl BlobDelay {
    pub fn dummy(shard_count: NonZeroUsize) -> Self {
        let lag_receivers = vec![
            {
                let (_, ch) = watch::channel(Duration::new(0, 0));
                ch
            };
            shard_count.into()
        ];
        Self {
            lag_receivers,
            entity: None,
        }
    }

    #[cfg(fbcode_build)]
    pub fn from_channels(lag_receivers: Vec<watch::Receiver<Duration>>, name: String) -> Self {
        let entity = Some(name);
        Self {
            lag_receivers,
            entity,
        }
    }

    pub async fn delay(&self, shard_id: usize) {
        let mut lag_receiver =
            tokio_stream::wrappers::WatchStream::new(self.lag_receivers[shard_id].clone());
        let start_time = Instant::now();

        while let Some(raw_lag) = lag_receiver.next().await {
            if raw_lag < MAX_LAG {
                if start_time.elapsed() > Duration::from_secs(1) {
                    // No jittering for short delays, but jitter us about a bit if we've seen
                    // lag and waited for it to die down, so that next request is random
                    jitter_delay(raw_lag).await;
                }
                break;
            }
            if let Some(entity) = &self.entity {
                let raw_lag_ms = raw_lag.as_millis().try_into();
                if let Ok(raw_lag_ms) = raw_lag_ms {
                    STATS::raw_lag_ms.add_value(raw_lag_ms, (entity.clone(),))
                }
            }
        }
        if let Some(entity) = &self.entity {
            let total_delay_ms = start_time.elapsed().as_millis().try_into();
            if let Ok(total_delay_ms) = total_delay_ms {
                STATS::total_delay_ms.add_value(total_delay_ms, (entity.clone(),));
            }
        }
    }
}
