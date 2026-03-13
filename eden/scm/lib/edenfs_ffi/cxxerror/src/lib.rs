/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// Error wrapper that preserves the full `anyhow` error chain across the CXX
/// bridge. CXX converts Rust errors to C++ exceptions using `Display`, which
/// for `anyhow::Error` only renders the outermost context. This wrapper's
/// `Display` uses the alternate format (`{:#}`) to include all causes.
pub struct CxxAnyhowError(anyhow::Error);

impl std::fmt::Display for CxxAnyhowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl<E: Into<anyhow::Error>> From<E> for CxxAnyhowError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}

/// Convenience alias for `Result<T, CxxAnyhowError>`.
pub type Result<T> = std::result::Result<T, CxxAnyhowError>;
