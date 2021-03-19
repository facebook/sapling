/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{MultiRendezVousController, RendezVousController};

#[derive(Copy, Clone)]
pub struct TunablesMultiRendezVousController;

impl MultiRendezVousController for TunablesMultiRendezVousController {
    type Controller = TunablesRendezVousController;

    fn new_controller(&self) -> Self::Controller {
        TunablesRendezVousController::new()
    }
}

pub struct TunablesRendezVousController {
    histogram: Mutex<MiniHistogram>,
}

impl TunablesRendezVousController {
    pub fn new() -> Self {
        Self {
            histogram: Mutex::new(MiniHistogram::new(Instant::now())),
        }
    }

    pub fn dispatch_delay(&self) -> Duration {
        Duration::from_millis(
            ::tunables::tunables()
                .get_rendezvous_dispatch_delay_ms()
                .try_into()
                .unwrap_or(0),
        )
    }
}

#[async_trait::async_trait]
impl RendezVousController for TunablesRendezVousController {
    /// We batch if we received more requests than our minimum threshold in any of the last 2
    /// batching windows. Concretely, this means that we only batch if we do expect that we'll be
    /// able to actually build batches of our desired size given the query arrival rate we are
    /// observing.
    fn should_batch(&self) -> bool {
        let threshold = ::tunables::tunables()
            .get_rendezvous_dispatch_min_threshold()
            .try_into()
            .unwrap_or(0);

        let window = self.dispatch_delay();

        let mut hist = self.histogram.lock().unwrap();
        should_batch(threshold, window, &mut *hist)
    }

    /// Wait for the configured dispatch delay.
    async fn wait_for_dispatch(&self) {
        tokio::time::delay_for(self.dispatch_delay()).await;
    }

    fn early_dispatch_threshold(&self) -> usize {
        ::tunables::tunables()
            .get_rendezvous_dispatch_max_threshold()
            .try_into()
            .unwrap_or(0)
    }
}

fn should_batch(min_threshold: usize, window: Duration, hist: &mut MiniHistogram) -> bool {
    hist.update(Instant::now(), window, 1);

    if min_threshold < hist.current || min_threshold < hist.last {
        return true;
    }

    false
}

struct MiniHistogram {
    last: usize,
    current: usize,
    boundary: Instant,
}

impl MiniHistogram {
    pub fn new(boundary: Instant) -> Self {
        Self {
            last: 0,
            current: 0,
            boundary,
        }
    }

    pub fn update(&mut self, instant: Instant, interval: Duration, n: usize) {
        if let Some(duration) = instant.checked_duration_since(self.boundary) {
            if duration > interval {
                self.last = self.current;
                self.current = 0;
                self.boundary = instant;
            }

            self.current += n;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_mini_histogram() {
        let t0 = Instant::now();
        let t5 = t0 + Duration::from_secs(5);
        let t10 = t0 + Duration::from_secs(10);
        let t20 = t0 + Duration::from_secs(20);

        let mut hist = MiniHistogram::new(t0);

        hist.update(t5, Duration::from_secs(15), 1);
        assert_eq!(hist.current, 1);
        assert_eq!(hist.last, 0);

        hist.update(t10, Duration::from_secs(15), 2);
        assert_eq!(hist.current, 3);
        assert_eq!(hist.last, 0);

        hist.update(t20, Duration::from_secs(15), 5);
        assert_eq!(hist.current, 5);
        assert_eq!(hist.last, 3);
        assert_eq!(hist.boundary, t20);

        hist.update(t10, Duration::from_secs(15), 5);
        assert_eq!(hist.current, 5);
        assert_eq!(hist.last, 3);
        assert_eq!(hist.boundary, t20);
    }

    #[test]
    fn test_should_batch() {
        let mut hist = MiniHistogram::new(Instant::now());
        let threshold = 2;
        let window = Duration::from_secs(10);

        assert!(!should_batch(threshold, window, &mut hist));
        assert!(!should_batch(threshold, window, &mut hist));
        assert!(should_batch(threshold, window, &mut hist));
    }
}
