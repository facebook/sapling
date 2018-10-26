// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate eventsource;
#[macro_use]
extern crate failure;
extern crate ini;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate regex;
extern crate reqwest;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

pub(crate) mod action;
pub mod config;
pub mod error;
pub mod receiver;
pub mod subscriber;
pub(crate) mod util;

pub use config::CommitCloudConfig;
pub use receiver::TcpReceiverService as CommitCloudTcpReceiverService;
pub use subscriber::WorkspaceSubscriberService as CommitCloudWorkspaceSubscriberService;

#[cfg(test)]
pub mod tests;

#[cfg(test)]
extern crate tempfile;
