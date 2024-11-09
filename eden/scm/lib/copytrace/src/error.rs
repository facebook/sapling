/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use dag::Vertex;
use types::HgId;
use types::RepoPathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CopyTraceError {
    /// Root tree id can not be found for the given commit
    #[error("Root tree id can not be found for commit: {0:?}")]
    RootTreeIdNotFound(HgId),

    /// Parents can not be found for the given commit
    #[error("Parents can not be found for commit: {0:?}")]
    NoParents(Vertex),

    #[error("File not found: {0:?}")]
    FileNotFound(RepoPathBuf),
}
