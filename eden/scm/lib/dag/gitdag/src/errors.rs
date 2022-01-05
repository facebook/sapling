/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub trait MapDagError<T> {
    fn context(self, message: &'static str) -> dag::Result<T>;
    fn with_context(self, func: impl Fn() -> String) -> dag::Result<T>;
}

impl<T> MapDagError<T> for Result<T, git2::Error> {
    fn context(self, message: &'static str) -> dag::Result<T> {
        anyhow::Context::context(self, message)
            .map_err(|e| dag::errors::BackendError::Other(e).into())
    }
    fn with_context(self, func: impl Fn() -> String) -> dag::Result<T> {
        anyhow::Context::with_context(self, func)
            .map_err(|e| dag::errors::BackendError::Other(e).into())
    }
}
