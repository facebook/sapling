/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Daemon-independent building blocks for EdenFS CLI workflows.
//!
//! This crate hosts the parts of the EdenFS CLI that manage on-disk state
//! (configs, checkouts, redirections) without requiring fbinit or the C++
//! thrift transport stack, so binaries like `sl` can link them directly.
//! The daemon calls those workflows need go through the narrow
//! [`daemon::EdenFsDaemon`] trait.

pub mod checkout;
pub mod daemon;
pub mod fsutil;
pub mod instance;
pub(crate) mod mounttable;
pub mod redirect;
pub mod unmount;
pub mod utils;
