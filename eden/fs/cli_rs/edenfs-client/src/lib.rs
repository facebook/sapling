/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use thrift_types::edenfs::client::EdenService;

pub mod checkout;
pub mod instance;
mod mounttable;
pub mod redirect;
mod utils;

pub use instance::DaemonHealthy;
pub use instance::EdenFsInstance;

pub type EdenFsClient = Arc<dyn EdenService + Sync>;
