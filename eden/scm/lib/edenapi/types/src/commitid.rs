/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Debug;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::HgId;

#[auto_wire]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum CommitIdScheme {
    #[id(1)]
    Hg,
    #[id(2)]
    Bonsai,
    #[id(3)]
    Globalrev,
    #[id(4)]
    GitSha1,
}

impl Default for CommitIdScheme {
    fn default() -> Self {
        Self::Hg
    }
}

sized_hash!(GitSha1, 20);
blake2_hash!(BonsaiChangesetId);

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum CommitId {
    #[id(1)]
    Hg(HgId),
    #[id(2)]
    Bonsai(BonsaiChangesetId),
    #[id(3)]
    Globalrev(u64),
    #[id(4)]
    GitSha1(GitSha1),
}

impl Default for CommitId {
    fn default() -> CommitId {
        CommitId::Hg(HgId::null_id().clone())
    }
}

impl fmt::Display for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CommitId::Hg(id) => std::fmt::Display::fmt(&id, f),
            CommitId::Bonsai(id) => std::fmt::Display::fmt(&id, f),
            CommitId::Globalrev(id) => std::fmt::Display::fmt(&id, f),
            CommitId::GitSha1(id) => std::fmt::Display::fmt(&id, f),
        }
    }
}
