/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::fetch_cause::FetchCause;
use crate::fetch_mode::FetchMode;

/// A context for a fetch operation.
/// The structure is extendable to support more context in the future
/// (e.g. cause of the fetch, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchContext {
    mode: FetchMode,
    cause: FetchCause,
}

impl FetchContext {
    pub fn new(mode: FetchMode) -> Self {
        Self {
            mode,
            cause: FetchCause::Unspecified,
        }
    }

    pub fn new_with_cause(mode: FetchMode, cause: FetchCause) -> Self {
        Self { mode, cause }
    }

    pub fn mode(&self) -> &FetchMode {
        &self.mode
    }
}

impl Default for FetchContext {
    fn default() -> Self {
        Self::new(FetchMode::AllowRemote)
    }
}
