// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

/// A changeset ID.  This is the canonical ID for a changeset.
pub type ChangesetId = mononoke_types::ChangesetId;

/// A changeset specifier.  This is anything that may be used to specify a
/// unique changeset, such as its bonsai changeset ID, a changeset hash in an
/// alternative hashing scheme, a globally unique hash prefix, or an
/// externally-assigned numerical ID.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum ChangesetSpecifier {
    Bonsai(ChangesetId),
}

impl fmt::Display for ChangesetSpecifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ChangesetSpecifier::Bonsai(cs_id) => write!(f, "changeset {}", cs_id),
        }
    }
}
