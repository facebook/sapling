/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub trait MapDagError<T> {
    fn context(self, message: &'static str) -> dag::Result<T>;
    fn with_context(self, func: impl Fn() -> String) -> dag::Result<T>;
}

impl<T> MapDagError<T> for Result<T, anyhow::Error> {
    fn context(self, message: &'static str) -> dag::Result<T> {
        anyhow::Context::context(self, message)
            .map_err(|e| dag::errors::BackendError::Other(e).into())
    }
    fn with_context(self, func: impl Fn() -> String) -> dag::Result<T> {
        anyhow::Context::with_context(self, func)
            .map_err(|e| dag::errors::BackendError::Other(e).into())
    }
}
