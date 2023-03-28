/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::Vertex;
use types::HgId;

#[derive(Debug, thiserror::Error)]
pub enum CopyTraceError {
    /// Root tree id can not be found for the given commit
    #[error("Root tree id can not be found for commit: {0:?}")]
    RootTreeIdNotFound(HgId),

    /// Parents can not be found for the given commit
    #[error("Parents can not be found for commit: {0:?}")]
    NoParents(Vertex),
}
