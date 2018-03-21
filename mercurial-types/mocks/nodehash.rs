// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types::{HgChangesetId, HgManifestId, NodeHash};
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
pub const ONES_CSID: HgChangesetId = HgChangesetId::new(ONES_HASH);
pub const TWOS_CSID: HgChangesetId = HgChangesetId::new(TWOS_HASH);
pub const THREES_CSID: HgChangesetId = HgChangesetId::new(THREES_HASH);
pub const FOURS_CSID: HgChangesetId = HgChangesetId::new(FOURS_HASH);
pub const FIVES_CSID: HgChangesetId = HgChangesetId::new(FIVES_HASH);
pub const SIXES_CSID: HgChangesetId = HgChangesetId::new(SIXES_HASH);
pub const SEVENS_CSID: HgChangesetId = HgChangesetId::new(SEVENS_HASH);
pub const EIGHTS_CSID: HgChangesetId = HgChangesetId::new(EIGHTS_HASH);
pub const NINES_CSID: HgChangesetId = HgChangesetId::new(NINES_HASH);
pub const AS_CSID: HgChangesetId = HgChangesetId::new(AS_HASH);
pub const BS_CSID: HgChangesetId = HgChangesetId::new(BS_HASH);
pub const CS_CSID: HgChangesetId = HgChangesetId::new(CS_HASH);
pub const DS_CSID: HgChangesetId = HgChangesetId::new(DS_HASH);
pub const ES_CSID: HgChangesetId = HgChangesetId::new(ES_HASH);
pub const FS_CSID: HgChangesetId = HgChangesetId::new(FS_HASH);

// Definitions for manifest IDs 1111...ffff
pub const ONES_MID: HgManifestId = HgManifestId::new(ONES_HASH);
pub const TWOS_MID: HgManifestId = HgManifestId::new(TWOS_HASH);
pub const THREES_MID: HgManifestId = HgManifestId::new(THREES_HASH);
pub const FOURS_MID: HgManifestId = HgManifestId::new(FOURS_HASH);
pub const FIVES_MID: HgManifestId = HgManifestId::new(FIVES_HASH);
pub const SIXES_MID: HgManifestId = HgManifestId::new(SIXES_HASH);
pub const SEVENS_MID: HgManifestId = HgManifestId::new(SEVENS_HASH);
pub const EIGHTS_MID: HgManifestId = HgManifestId::new(EIGHTS_HASH);
pub const NINES_MID: HgManifestId = HgManifestId::new(NINES_HASH);
pub const AS_MID: HgManifestId = HgManifestId::new(AS_HASH);
pub const BS_MID: HgManifestId = HgManifestId::new(BS_HASH);
pub const CS_MID: HgManifestId = HgManifestId::new(CS_HASH);
pub const DS_MID: HgManifestId = HgManifestId::new(DS_HASH);
pub const ES_MID: HgManifestId = HgManifestId::new(ES_HASH);
pub const FS_MID: HgManifestId = HgManifestId::new(FS_HASH);
