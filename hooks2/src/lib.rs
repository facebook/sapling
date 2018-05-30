// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This crate contains the core structs and traits that implement the hook subsystem in
//! Mononoke.
//! Hooks are user defined pieces of code, typically written in a scripting language that
//! can be run at different stages of the process of rebasing user changes into a server side
//! bookmark.
//! The scripting language specific implementation of hooks are in the corresponding sub module.

#![deny(warnings)]

extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate hlua;
extern crate hlua_futures;

#[cfg(test)]
extern crate async_unit;
#[cfg(test)]
extern crate linear;
#[cfg(test)]
extern crate tempdir;
#[cfg(test)]
extern crate tokio_core;

pub mod lua_hook;
pub mod rust_hook;

use failure::{Error, Result};
use futures_ext::BoxFuture;
use std::sync::Arc;

/// Represents something that knows how to run a hook
pub trait HookRunner {
    fn run_hook(
        &self,
        hook: Box<Hook>,
        changeset: Arc<HookChangeset>,
    ) -> BoxFuture<bool, Error>;
}

pub trait Hook: Send {
    fn run(&self, changeset: Arc<HookChangeset>) -> Result<bool>;
}

/// Represents a changeset - more user friendly than the blob changeset
/// as this uses String not Vec[u8]
pub struct HookChangeset {
    pub user: String,
    pub files: Vec<String>,
}

impl HookChangeset {
    pub fn new(user: String, files: Vec<String>) -> HookChangeset {
        HookChangeset { user, files }
    }
}
