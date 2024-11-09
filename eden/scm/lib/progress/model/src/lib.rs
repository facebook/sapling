/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
pub use progress_bar::ActiveProgressBar;
pub use progress_bar::AggregatingProgressBar;
pub use progress_bar::BarState;
pub use progress_bar::Builder as ProgressBarBuilder;
pub use progress_bar::ProgressBar;
pub use registry::Registry;
pub use time_series::IoTimeSeries;
pub use time_series::TimeSeriesMode;
