/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Data model related to progress reporting.
//!
//! - Pure data. Minimal state just enough for rendering.
//! - Separate from rendering.
//! - Lock-free (nice to have).

mod cache_stats;
mod io_sample;
mod progress_bar;
mod registry;
mod time_series;

pub use cache_stats::CacheStats;
pub use io_sample::IoSample;
pub use progress_bar::AggregatingProgressBar;
pub use progress_bar::ProgressBar;
pub use registry::Registry;
pub use time_series::IoTimeSeries;
pub use time_series::TimeSeriesMode;
