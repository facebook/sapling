/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Client-server with the ability to preload content server-side to reduce
//! startup overhead.

pub mod client;
pub mod ipc;
pub mod server;
mod spawn;
mod util;
