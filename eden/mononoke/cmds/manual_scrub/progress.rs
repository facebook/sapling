/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use derive_more::Add;
use derive_more::Sub;
use slog::info;
use slog::Logger;
use std::fmt;
use std::time::Instant;

#[derive(Add, Sub, Clone, Copy, Default, Debug)]
pub struct Progress {
    pub success: u64,
    pub missing: u64,
    pub error: u64,
    pub skipped: u64,
    pub bytes: u64,
}

// Log at most every N seconds
const PROGRESS_INTERVAL_SECS: u64 = 30;

impl fmt::Display for Progress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}, {}, {}, {}, {}, {}",
            self.success,
            self.missing,
            self.error,
            self.total(),
            self.skipped,
            self.bytes,
        )
    }
}

impl Progress {
    pub fn total(&self) -> u64 {
        self.success + self.missing + self.error
    }

    pub fn legend(&self, logger: &Logger) {
        info!(
            logger,
            "period, rate/s, seconds, success, missing, error, total, skipped, bytes, bytes/s"
        );
    }

    // Returns time of last log, if any
    pub fn record(
        &self,
        logger: &Logger,
        quiet: bool,
        started: Instant,
        prev: Option<(Progress, Instant)>,
        is_final: bool,
    ) -> Result<Option<Instant>, Error> {
        let log_period = |period, run: &Self, period_secs| {
            let per_sec = if period_secs > 0 {
                run.total() / period_secs
            } else {
                0
            };
            let bytes_per_sec = if period_secs > 0 {
                run.bytes / period_secs
            } else {
                0
            };
            info!(
                logger,
                "{}, {:06}, {}, {}, {}", period, per_sec, period_secs, run, bytes_per_sec
            );
        };

        let now = Instant::now();
        let run_secs = now.duration_since(started).as_secs();

        if let Some((prev, prev_t)) = prev {
            // keep log volume down
            let delta_secs = now.duration_since(prev_t).as_secs();
            if delta_secs < PROGRESS_INTERVAL_SECS && !is_final {
                return Ok(None);
            }
            if !quiet {
                log_period("run", self, run_secs);
                let delta = *self - prev;
                log_period("delta", &delta, delta_secs);
            }
        } else if !quiet {
            log_period("run", self, run_secs);
        }
        Ok(Some(now))
    }
}
