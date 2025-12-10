/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use faster_hex::hex_string;

use crate::types::CommitId;
use crate::types::RepoSpecifier;

impl fmt::Display for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CommitId::bonsai(bonsai) => write!(f, "{}", hex_string(bonsai)),
            CommitId::hg(hg) => write!(f, "{}", hex_string(hg)),
            CommitId::git(git) => write!(f, "{}", hex_string(git)),
            CommitId::globalrev(rev) => write!(f, "{}", rev),
            CommitId::svnrev(rev) => write!(f, "{}", rev),
            CommitId::ephemeral_bonsai(ephemeral) => {
                write!(
                    f,
                    "{} (bubble {})",
                    hex_string(&ephemeral.bonsai_id),
                    ephemeral.bubble_id
                )
            }
            CommitId::UnknownField(t) => write!(f, "unknown id type ({})", t),
        }
    }
}

impl fmt::Display for RepoSpecifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}
