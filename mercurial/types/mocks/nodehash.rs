// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Ignore deprecation of HgNodeHash::new
#![allow(deprecated)]

use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId, HgNodeHash};
// NULL_HASH is exported for convenience.
pub use mercurial_types::NULL_HASH;

use crate::hash;

// Definitions for hashes 1111...ffff.
pub const ONES_HASH: HgNodeHash = HgNodeHash::new(hash::ONES);
pub const TWOS_HASH: HgNodeHash = HgNodeHash::new(hash::TWOS);
pub const THREES_HASH: HgNodeHash = HgNodeHash::new(hash::THREES);
pub const FOURS_HASH: HgNodeHash = HgNodeHash::new(hash::FOURS);
pub const FIVES_HASH: HgNodeHash = HgNodeHash::new(hash::FIVES);
pub const SIXES_HASH: HgNodeHash = HgNodeHash::new(hash::SIXES);
pub const SEVENS_HASH: HgNodeHash = HgNodeHash::new(hash::SEVENS);
pub const EIGHTS_HASH: HgNodeHash = HgNodeHash::new(hash::EIGHTS);
pub const NINES_HASH: HgNodeHash = HgNodeHash::new(hash::NINES);
pub const AS_HASH: HgNodeHash = HgNodeHash::new(hash::AS);
pub const BS_HASH: HgNodeHash = HgNodeHash::new(hash::BS);
pub const CS_HASH: HgNodeHash = HgNodeHash::new(hash::CS);
pub const DS_HASH: HgNodeHash = HgNodeHash::new(hash::DS);
pub const ES_HASH: HgNodeHash = HgNodeHash::new(hash::ES);
pub const FS_HASH: HgNodeHash = HgNodeHash::new(hash::FS);

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

// Definitions for filenode IDs 1111...ffff
pub const ONES_FNID: HgFileNodeId = HgFileNodeId::new(ONES_HASH);
pub const TWOS_FNID: HgFileNodeId = HgFileNodeId::new(TWOS_HASH);
pub const THREES_FNID: HgFileNodeId = HgFileNodeId::new(THREES_HASH);
pub const FOURS_FNID: HgFileNodeId = HgFileNodeId::new(FOURS_HASH);
pub const FIVES_FNID: HgFileNodeId = HgFileNodeId::new(FIVES_HASH);
pub const SIXES_FNID: HgFileNodeId = HgFileNodeId::new(SIXES_HASH);
pub const SEVENS_FNID: HgFileNodeId = HgFileNodeId::new(SEVENS_HASH);
pub const EIGHTS_FNID: HgFileNodeId = HgFileNodeId::new(EIGHTS_HASH);
pub const NINES_FNID: HgFileNodeId = HgFileNodeId::new(NINES_HASH);
pub const AS_FNID: HgFileNodeId = HgFileNodeId::new(AS_HASH);
pub const BS_FNID: HgFileNodeId = HgFileNodeId::new(BS_HASH);
pub const CS_FNID: HgFileNodeId = HgFileNodeId::new(CS_HASH);
pub const DS_FNID: HgFileNodeId = HgFileNodeId::new(DS_HASH);
pub const ES_FNID: HgFileNodeId = HgFileNodeId::new(ES_HASH);
pub const FS_FNID: HgFileNodeId = HgFileNodeId::new(FS_HASH);
