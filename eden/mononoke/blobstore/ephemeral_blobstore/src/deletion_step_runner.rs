/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Runs the steps of bubble deletions, with debug logging of their progress.

use std::future::Future;
use std::pin::pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use tracing::debug;

use crate::bubble::BubbleId;

/// Number of bubble deletions in a process that log every step. Later
/// deletions only log slow steps, to keep the log size bounded.
const DETAILED_DELETIONS: usize = 200;

/// Steps of any deletion that take longer than this are logged.
const SLOW_STEP_THRESHOLD: Duration = Duration::from_secs(60);

static DELETIONS_STARTED: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct DeletionStepRunner {
    bubble_id: BubbleId,
    detailed: bool,
}

impl DeletionStepRunner {
    pub(crate) fn new(bubble_id: BubbleId) -> Self {
        let detailed = DELETIONS_STARTED.fetch_add(1, Ordering::Relaxed) < DETAILED_DELETIONS;
        Self {
            bubble_id,
            detailed,
        }
    }

    /// Runs one step of the deletion, logging its start if this deletion is
    /// detailed, and logging it if it is slow.
    pub(crate) async fn run_step<T>(&self, step: &str, fut: impl Future<Output = T>) -> T {
        let bubble_id = self.bubble_id;
        if self.detailed {
            debug!(%bubble_id, step, "Bubble deletion step started");
        }
        let started = Instant::now();
        let mut fut = pin!(fut);
        if let Ok(output) = tokio::time::timeout(SLOW_STEP_THRESHOLD, fut.as_mut()).await {
            return output;
        }
        debug!(%bubble_id, step, "Bubble deletion step still running after {}s", SLOW_STEP_THRESHOLD.as_secs());
        let output = fut.await;
        debug!(%bubble_id, step, "Bubble deletion step finished after {}s", started.elapsed().as_secs());
        output
    }
}
