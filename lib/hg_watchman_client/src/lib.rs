// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate failure;
extern crate serde;
#[macro_use]
extern crate serde_json;
extern crate watchman_client;

mod hgclient;
pub use crate::hgclient::HgWatchmanClient;
pub use watchman_client::queries::*;
