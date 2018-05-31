// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! This sub module contains a simple Rust implementation of hooks
//! This implementation is meant for testing and experimentation, not for real usage

#![deny(warnings)]

use super::Hook;
use super::HookChangeset;

use failure::Result;
use std::sync::Arc;

pub struct RustHook {
    pub name: String,
}

impl Hook for RustHook {
    fn run(&self, changeset: Arc<HookChangeset>) -> Result<bool> {
        println!("Running hook {}", self.name);
        println!("Changeset user is {}", changeset.user);
        (*changeset)
            .files
            .iter()
            .for_each(|file| println!("{}", file));
        Ok(true)
    }
}
