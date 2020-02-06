/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::convert::From;
use std::fmt;

/// A changeset ID.  This is the canonical ID for a changeset.
pub type ChangesetId = mononoke_types::ChangesetId;

/// A Mercurial changeset ID.
pub type HgChangesetId = mercurial_types::HgChangesetId;

/// A Globalrev.
pub type Globalrev = mercurial_types::Globalrev;

/// A changeset specifier.  This is anything that may be used to specify a
/// unique changeset, such as its bonsai changeset ID, a changeset hash in an
/// alternative hashing scheme, a globally unique hash prefix, or an
/// externally-assigned numerical ID.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum ChangesetSpecifier {
    Bonsai(ChangesetId),
    Hg(HgChangesetId),
    Globalrev(Globalrev),
}

/// This is prefix that may be used to resolve a changeset
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum ChangesetIdPrefix<'a> {
    BonsaiHexPrefix(&'a str),
    HgHexPrefix(&'a str),
}

/// This is the result of resolving changesets by prefix
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum ChangesetIdPrefixResolution {
    NoMatch,
    Single(ChangesetSpecifier),
    Multiple(Vec<ChangesetSpecifier>),
    TooMany(Vec<ChangesetSpecifier>),
}

impl From<mercurial_types::HgChangesetIdsResolvedFromPrefix> for ChangesetIdPrefixResolution {
    fn from(resolved: mercurial_types::HgChangesetIdsResolvedFromPrefix) -> Self {
        use mercurial_types::HgChangesetIdsResolvedFromPrefix::*;
        use ChangesetSpecifier::*;
        match resolved {
            Single(id) => Self::Single(Hg(id)),
            Multiple(ids) => Self::Multiple(ids.into_iter().map(|id| Hg(id)).collect()),
            TooMany(ids) => Self::TooMany(ids.into_iter().map(|id| Hg(id)).collect()),
            NoMatch => Self::NoMatch,
        }
    }
}

impl From<mononoke_types::ChangesetIdsResolvedFromPrefix> for ChangesetIdPrefixResolution {
    fn from(resolved: mononoke_types::ChangesetIdsResolvedFromPrefix) -> Self {
        use mononoke_types::ChangesetIdsResolvedFromPrefix::*;
        use ChangesetSpecifier::*;
        match resolved {
            Single(id) => Self::Single(Bonsai(id)),
            Multiple(ids) => Self::Multiple(ids.into_iter().map(|id| Bonsai(id)).collect()),
            TooMany(ids) => Self::TooMany(ids.into_iter().map(|id| Bonsai(id)).collect()),
            NoMatch => Self::NoMatch,
        }
    }
}

impl fmt::Display for ChangesetSpecifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ChangesetSpecifier::Bonsai(cs_id) => write!(f, "changeset {}", cs_id),
            ChangesetSpecifier::Hg(hg_cs_id) => write!(f, "hg changeset {}", hg_cs_id),
            ChangesetSpecifier::Globalrev(rev) => write!(f, "globalrev {}", rev.id()),
        }
    }
}
