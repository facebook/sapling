/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use dag::Vertex;
use types::RepoPathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CopyTraceError {
    /// Parents can not be found for the given commit
    #[error("Parents can not be found for commit: {0:?}")]
    NoParents(Vertex),

    #[error("File not found: {0:?}")]
    FileNotFound(RepoPathBuf),
}
