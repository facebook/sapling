/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Client-server with the ability to preload content server-side to reduce
//! startup overhead.

pub mod client;
pub mod ipc;
pub mod server;
mod spawn;
mod util;
