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
use super::HookRunner;

use failure::{Error, Result};
use futures_ext::{asynchronize, BoxFuture};
use std::sync::Arc;

pub struct RustHookRunner {}

pub struct RustHook {
    pub name: String,
}

impl HookRunner for RustHookRunner {
    fn run_hook(
        self: &Self,
        hook: Box<Hook>,
        changeset: Arc<HookChangeset>,
    ) -> BoxFuture<bool, Error> {
        let fut = asynchronize(move || hook.run(changeset));
        Box::new(fut)
    }
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

impl RustHook {
    pub fn new(name: String) -> RustHook {
        RustHook { name }
    }
}
