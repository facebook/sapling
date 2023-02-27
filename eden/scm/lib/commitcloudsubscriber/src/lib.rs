/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub(crate) mod action;
pub mod config;
pub mod error;
pub mod receiver;
pub mod subscriber;
pub(crate) mod util;

pub use config::CommitCloudConfig;
pub use receiver::TcpReceiverService as CommitCloudTcpReceiverService;
pub use subscriber::WorkspaceSubscriberService as CommitCloudWorkspaceSubscriberService;

pub(crate) type ActionsMap =
    std::collections::HashMap<receiver::CommandName, Box<dyn Fn() + Send + Sync>>;

#[cfg(test)]
pub mod tests;
