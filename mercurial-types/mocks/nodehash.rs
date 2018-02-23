// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types::{ChangesetId, NodeHash};
// NULL_HASH is exported for convenience.
pub use mercurial_types::NULL_HASH;

use hash;

// Definitions for hashes 1111...ffff.
pub const ONES_HASH: NodeHash = NodeHash::new(hash::ONES);
pub const TWOS_HASH: NodeHash = NodeHash::new(hash::TWOS);
pub const THREES_HASH: NodeHash = NodeHash::new(hash::THREES);
pub const FOURS_HASH: NodeHash = NodeHash::new(hash::FOURS);
pub const FIVES_HASH: NodeHash = NodeHash::new(hash::FIVES);
pub const SIXES_HASH: NodeHash = NodeHash::new(hash::SIXES);
pub const SEVENS_HASH: NodeHash = NodeHash::new(hash::SEVENS);
pub const EIGHTS_HASH: NodeHash = NodeHash::new(hash::EIGHTS);
pub const NINES_HASH: NodeHash = NodeHash::new(hash::NINES);
pub const AS_HASH: NodeHash = NodeHash::new(hash::AS);
pub const BS_HASH: NodeHash = NodeHash::new(hash::BS);
pub const CS_HASH: NodeHash = NodeHash::new(hash::CS);
pub const DS_HASH: NodeHash = NodeHash::new(hash::DS);
pub const ES_HASH: NodeHash = NodeHash::new(hash::ES);
pub const FS_HASH: NodeHash = NodeHash::new(hash::FS);

// Definitions for changeset IDs 1111...ffff
pub const ONES_CSID: ChangesetId = ChangesetId::new(ONES_HASH);
pub const TWOS_CSID: ChangesetId = ChangesetId::new(TWOS_HASH);
pub const THREES_CSID: ChangesetId = ChangesetId::new(THREES_HASH);
pub const FOURS_CSID: ChangesetId = ChangesetId::new(FOURS_HASH);
pub const FIVES_CSID: ChangesetId = ChangesetId::new(FIVES_HASH);
pub const SIXES_CSID: ChangesetId = ChangesetId::new(SIXES_HASH);
pub const SEVENS_CSID: ChangesetId = ChangesetId::new(SEVENS_HASH);
pub const EIGHTS_CSID: ChangesetId = ChangesetId::new(EIGHTS_HASH);
pub const NINES_CSID: ChangesetId = ChangesetId::new(NINES_HASH);
pub const AS_CSID: ChangesetId = ChangesetId::new(AS_HASH);
pub const BS_CSID: ChangesetId = ChangesetId::new(BS_HASH);
pub const CS_CSID: ChangesetId = ChangesetId::new(CS_HASH);
pub const DS_CSID: ChangesetId = ChangesetId::new(DS_HASH);
pub const ES_CSID: ChangesetId = ChangesetId::new(ES_HASH);
pub const FS_CSID: ChangesetId = ChangesetId::new(FS_HASH);
