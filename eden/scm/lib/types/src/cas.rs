/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use crate::Blake3;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct CasDigest {
    pub hash: Blake3,
    pub size: u64,
}

// CAS is agnostic to the type of the digest, however this is useful for logging
#[derive(Debug, Clone, Copy)]
pub enum CasDigestType {
    Tree,
    File,
    Mixed,
}

impl fmt::Display for CasDigestType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CasDigestType::Tree => write!(f, "tree"),
            CasDigestType::File => write!(f, "file"),
            CasDigestType::Mixed => write!(f, "digest"),
        }
    }
}
