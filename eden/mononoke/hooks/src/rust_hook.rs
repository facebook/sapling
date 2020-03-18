/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This sub module contains a simple Rust implementation of hooks
//! This implementation is meant for testing and experimentation, not for real usage

#![deny(warnings)]

use super::{Hook, HookChangeset, HookContext, HookExecution};
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;

pub struct RustHook {
    pub name: String,
}

#[async_trait]
impl Hook<HookChangeset> for RustHook {
    async fn run(
        &self,
        _ctx: &CoreContext,
        _context: HookContext<HookChangeset>,
    ) -> Result<HookExecution, Error> {
        Ok(HookExecution::Accepted)
    }
}
