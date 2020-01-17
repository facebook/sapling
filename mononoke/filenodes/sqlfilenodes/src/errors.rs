/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use mercurial_types::{HgFileNodeId, RepoPath};
use thiserror::Error;

#[derive(Debug, Eq, Error, PartialEq)]
pub enum ErrorKind {
    #[error("Invalid copy: {0:?} copied from {1:?}")]
    InvalidCopy(RepoPath, RepoPath),
    #[error("Internal error: failure while fetching file node {0} {1}")]
    FailFetchFilenode(HgFileNodeId, RepoPath),
    #[error("Internal error: failure while fetching copy information {0} {1}")]
    FailFetchCopydata(HgFileNodeId, RepoPath),
    #[error("Internal error: copy information is not found for {0} {1}")]
    CopydataNotFound(HgFileNodeId, RepoPath),
    #[error("Internal error: failure while fetching file nodes for {0}")]
    FailRangeFetch(RepoPath),
    #[error("Internal error: failure while fetching copy source path for {0}")]
    FromPathNotFound(RepoPath),
}
