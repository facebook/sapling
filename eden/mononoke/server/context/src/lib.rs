/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(atomic_min_max)]
#![cfg_attr(fbcode_build, feature(never_type))]

pub use session_id::SessionId;

pub use crate::core::CoreContext;
#[cfg(fbcode_build)]
pub use crate::facebook::prelude::*;
pub use crate::logging::LoggingContainer;
pub use crate::perf_counters::{PerfCounterType, PerfCounters};
pub use crate::session::{generate_session_id, SessionContainer};

mod core;
#[cfg(fbcode_build)]
mod facebook;
mod logging;
mod perf_counters;
mod session;
