/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[derive(Debug, thiserror::Error)]
#[error("checkout error: {source}")]
pub struct CheckoutError {
    pub resumable: bool,
    pub source: anyhow::Error,
}

#[derive(Debug, thiserror::Error)]
#[error("error updating {path}: {message}")]
pub struct EdenConflictError {
    pub path: String,
    pub message: String,
}

/// Local directories whose contents block a file at the same path in the
/// destination commit.
#[derive(Debug)]
pub struct DirectoryConflictsError {
    pub paths: Vec<types::RepoPathBuf>,
}

impl std::fmt::Display for DirectoryConflictsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "nonempty directories conflict with files in the destination commit:\n {}\n(remove the local files or goto --clean to discard them)",
            crate::truncated_error_list(&self.paths, 5).join("\n "),
        )
    }
}

impl std::error::Error for DirectoryConflictsError {}
