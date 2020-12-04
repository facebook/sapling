/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod context;
mod drain;

pub use context::ObservabilityContext;
pub use drain::DynamicLevelDrain;
