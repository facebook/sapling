/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod async_limiter_;
mod errors;
mod flavor;
mod rate_limit_stream;

pub use async_limiter_::AsyncLimiter;
pub use errors::ErrorKind;
pub use flavor::TokioFlavor;
pub use rate_limit_stream::{EarliestPossible, RateLimitStream};
