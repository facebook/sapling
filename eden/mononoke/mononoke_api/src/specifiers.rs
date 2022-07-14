/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStore;
use std::fmt;

/// A changeset ID.  This is the canonical ID for a changeset.
pub type ChangesetId = mononoke_types::ChangesetId;

/// A Mercurial changeset ID.
pub type HgChangesetId = mercurial_types::HgChangesetId;

/// A Globalrev.
pub type Globalrev = mercurial_types::Globalrev;

/// A Git SHA-1 hash.
pub type GitSha1 = mononoke_types::hash::GitSha1;

/// A SVN revision number.
pub type Svnrev = mononoke_types::Svnrev;

/// A changeset specifier.  This is anything that may be used to specify a
/// unique changeset, such as its bonsai changeset ID, a changeset hash in an
/// alternative hashing scheme, a globally unique hash prefix, or an
/// externally-assigned numerical ID.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum ChangesetSpecifier {
    Bonsai(ChangesetId),
    EphemeralBonsai(ChangesetId, Option<BubbleId>),
    Hg(HgChangesetId),
    Globalrev(Globalrev),
    GitSha1(GitSha1),
    Svnrev(Svnrev),
}

impl From<ChangesetId> for ChangesetSpecifier {
    fn from(id: ChangesetId) -> Self {
        Self::Bonsai(id)
    }
}

impl From<HgChangesetId> for ChangesetSpecifier {
    fn from(id: HgChangesetId) -> Self {
        Self::Hg(id)
    }
}

impl From<Globalrev> for ChangesetSpecifier {
    fn from(id: Globalrev) -> Self {
        Self::Globalrev(id)
    }
}

impl From<Svnrev> for ChangesetSpecifier {
    fn from(id: Svnrev) -> Self {
        Self::Svnrev(id)
    }
}

impl From<GitSha1> for ChangesetSpecifier {
    fn from(id: GitSha1) -> Self {
        Self::GitSha1(id)
    }
}

impl ChangesetSpecifier {
    pub fn in_bubble(&self) -> bool {
        use ChangesetSpecifier::*;
        match self {
            EphemeralBonsai(_, _) => true,
            Bonsai(_) | Hg(_) | Globalrev(_) | GitSha1(_) | Svnrev(_) => false,
        }
    }

    pub async fn bubble_id(
        &self,
        ephemeral_blobstore: RepoEphemeralStore,
    ) -> Result<Option<BubbleId>> {
        use ChangesetSpecifier::*;
        Ok(match self {
            EphemeralBonsai(cs_id, bubble_id) => Some(match bubble_id {
                Some(id) => id.clone(),
                None => ephemeral_blobstore
                    .bubble_from_changeset(cs_id)
                    .await?
                    .with_context(|| format!("changeset {} does not belong to bubble", cs_id))?,
            }),
            Bonsai(_) | Hg(_) | Globalrev(_) | GitSha1(_) | Svnrev(_) => None,
        })
    }
}

/// A prefix of canonical ID for a changeset (Bonsai).
pub type ChangesetIdPrefix = mononoke_types::ChangesetIdPrefix;

/// A prefix of a Mercurial changeset ID.
pub type HgChangesetIdPrefix = mercurial_types::HgChangesetIdPrefix;

/// This is prefix that may be used to resolve a changeset
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum ChangesetPrefixSpecifier {
    Bonsai(ChangesetIdPrefix),
    Hg(HgChangesetIdPrefix),
    Globalrev(Globalrev),
}

impl From<HgChangesetIdPrefix> for ChangesetPrefixSpecifier {
    fn from(prefix: HgChangesetIdPrefix) -> Self {
        Self::Hg(prefix)
    }
}

impl From<ChangesetIdPrefix> for ChangesetPrefixSpecifier {
    fn from(prefix: ChangesetIdPrefix) -> Self {
        Self::Bonsai(prefix)
    }
}

impl From<Globalrev> for ChangesetPrefixSpecifier {
    fn from(prefix: Globalrev) -> Self {
        Self::Globalrev(prefix)
    }
}

/// This is the result of resolving changesets by prefix
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub enum ChangesetSpecifierPrefixResolution {
    NoMatch,
    Single(ChangesetSpecifier),
    Multiple(Vec<ChangesetSpecifier>),
    TooMany(Vec<ChangesetSpecifier>),
}

impl ChangesetSpecifierPrefixResolution {
    pub fn into_list(self) -> Vec<ChangesetSpecifier> {
        use ChangesetSpecifierPrefixResolution::*;
        match self {
            NoMatch => Vec::new(),
            Single(x) => vec![x],
            Multiple(v) => v,
            TooMany(v) => v,
        }
    }
}

impl From<mercurial_types::HgChangesetIdsResolvedFromPrefix>
    for ChangesetSpecifierPrefixResolution
{
    fn from(resolved: mercurial_types::HgChangesetIdsResolvedFromPrefix) -> Self {
        use mercurial_types::HgChangesetIdsResolvedFromPrefix::*;
        use ChangesetSpecifier::*;
        match resolved {
            Single(id) => Self::Single(Hg(id)),
            Multiple(ids) => Self::Multiple(ids.into_iter().map(Hg).collect()),
            TooMany(ids) => Self::TooMany(ids.into_iter().map(Hg).collect()),
            NoMatch => Self::NoMatch,
        }
    }
}

impl From<mononoke_types::ChangesetIdsResolvedFromPrefix> for ChangesetSpecifierPrefixResolution {
    fn from(resolved: mononoke_types::ChangesetIdsResolvedFromPrefix) -> Self {
        use mononoke_types::ChangesetIdsResolvedFromPrefix::*;
        use ChangesetSpecifier::*;
        match resolved {
            Single(id) => Self::Single(Bonsai(id)),
            Multiple(ids) => Self::Multiple(ids.into_iter().map(Bonsai).collect()),
            TooMany(ids) => Self::TooMany(ids.into_iter().map(Bonsai).collect()),
            NoMatch => Self::NoMatch,
        }
    }
}

impl From<Option<Globalrev>> for ChangesetSpecifierPrefixResolution {
    fn from(resolved: Option<Globalrev>) -> Self {
        use ChangesetSpecifier::*;
        match resolved {
            Some(globalrev) => Self::Single(Globalrev(globalrev)),
            None => Self::NoMatch,
        }
    }
}

impl fmt::Display for ChangesetSpecifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ChangesetSpecifier::Bonsai(cs_id) => write!(f, "changeset {}", cs_id),
            ChangesetSpecifier::EphemeralBonsai(cs_id, bubble_id) => {
                write!(
                    f,
                    "ephemeral changeset {} in bubble {}",
                    cs_id,
                    bubble_id.map_or_else(|| "unknown".to_string(), |b| b.to_string())
                )
            }
            ChangesetSpecifier::Hg(hg_cs_id) => write!(f, "hg changeset {}", hg_cs_id),
            ChangesetSpecifier::Globalrev(rev) => write!(f, "globalrev {}", rev.id()),
            ChangesetSpecifier::GitSha1(git_sha1) => write!(f, "git sha1 {}", git_sha1),
            ChangesetSpecifier::Svnrev(rev) => write!(f, "svn rev {}", rev.id()),
        }
    }
}
