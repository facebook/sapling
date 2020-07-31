/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::Vertex;
use std::fmt;

#[derive(Debug)]
pub struct CommitNotFound(pub Vertex);

#[derive(Debug)]
pub struct RevNotFound(pub u32);

impl fmt::Display for CommitNotFound {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "00changelog.i@{:.8?}: not found", &self.0)
    }
}

impl std::error::Error for CommitNotFound {}

impl fmt::Display for RevNotFound {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "00changelog.i: rev {:.8?} not found", &self.0)
    }
}

impl std::error::Error for RevNotFound {}
