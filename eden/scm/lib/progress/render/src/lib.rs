/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Progress rendering.

mod config;
pub mod simple;
mod unit;

pub use config::RenderingConfig;

#[cfg(test)]
mod tests;
