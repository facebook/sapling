// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Ignore deprecation of HgNodeHash::new
#![allow(deprecated)]

use mercurial_types::{DChangesetId, DFileNodeId, DManifestId, DNodeHash};
// D_NULL_HASH is exported for convenience.
pub use mercurial_types::D_NULL_HASH;

use hash;

// Definitions for hashes 1111...ffff.
pub const ONES_HASH: DNodeHash = DNodeHash::new(hash::ONES);
pub const TWOS_HASH: DNodeHash = DNodeHash::new(hash::TWOS);
pub const THREES_HASH: DNodeHash = DNodeHash::new(hash::THREES);
pub const FOURS_HASH: DNodeHash = DNodeHash::new(hash::FOURS);
pub const FIVES_HASH: DNodeHash = DNodeHash::new(hash::FIVES);
pub const SIXES_HASH: DNodeHash = DNodeHash::new(hash::SIXES);
pub const SEVENS_HASH: DNodeHash = DNodeHash::new(hash::SEVENS);
pub const EIGHTS_HASH: DNodeHash = DNodeHash::new(hash::EIGHTS);
pub const NINES_HASH: DNodeHash = DNodeHash::new(hash::NINES);
pub const AS_HASH: DNodeHash = DNodeHash::new(hash::AS);
pub const BS_HASH: DNodeHash = DNodeHash::new(hash::BS);
pub const CS_HASH: DNodeHash = DNodeHash::new(hash::CS);
pub const DS_HASH: DNodeHash = DNodeHash::new(hash::DS);
pub const ES_HASH: DNodeHash = DNodeHash::new(hash::ES);
pub const FS_HASH: DNodeHash = DNodeHash::new(hash::FS);

// Definitions for changeset IDs 1111...ffff
pub const ONES_CSID: DChangesetId = DChangesetId::new(ONES_HASH);
pub const TWOS_CSID: DChangesetId = DChangesetId::new(TWOS_HASH);
pub const THREES_CSID: DChangesetId = DChangesetId::new(THREES_HASH);
pub const FOURS_CSID: DChangesetId = DChangesetId::new(FOURS_HASH);
pub const FIVES_CSID: DChangesetId = DChangesetId::new(FIVES_HASH);
pub const SIXES_CSID: DChangesetId = DChangesetId::new(SIXES_HASH);
pub const SEVENS_CSID: DChangesetId = DChangesetId::new(SEVENS_HASH);
pub const EIGHTS_CSID: DChangesetId = DChangesetId::new(EIGHTS_HASH);
pub const NINES_CSID: DChangesetId = DChangesetId::new(NINES_HASH);
pub const AS_CSID: DChangesetId = DChangesetId::new(AS_HASH);
pub const BS_CSID: DChangesetId = DChangesetId::new(BS_HASH);
pub const CS_CSID: DChangesetId = DChangesetId::new(CS_HASH);
pub const DS_CSID: DChangesetId = DChangesetId::new(DS_HASH);
pub const ES_CSID: DChangesetId = DChangesetId::new(ES_HASH);
pub const FS_CSID: DChangesetId = DChangesetId::new(FS_HASH);

// Definitions for manifest IDs 1111...ffff
pub const ONES_MID: DManifestId = DManifestId::new(ONES_HASH);
pub const TWOS_MID: DManifestId = DManifestId::new(TWOS_HASH);
pub const THREES_MID: DManifestId = DManifestId::new(THREES_HASH);
pub const FOURS_MID: DManifestId = DManifestId::new(FOURS_HASH);
pub const FIVES_MID: DManifestId = DManifestId::new(FIVES_HASH);
pub const SIXES_MID: DManifestId = DManifestId::new(SIXES_HASH);
pub const SEVENS_MID: DManifestId = DManifestId::new(SEVENS_HASH);
pub const EIGHTS_MID: DManifestId = DManifestId::new(EIGHTS_HASH);
pub const NINES_MID: DManifestId = DManifestId::new(NINES_HASH);
pub const AS_MID: DManifestId = DManifestId::new(AS_HASH);
pub const BS_MID: DManifestId = DManifestId::new(BS_HASH);
pub const CS_MID: DManifestId = DManifestId::new(CS_HASH);
pub const DS_MID: DManifestId = DManifestId::new(DS_HASH);
pub const ES_MID: DManifestId = DManifestId::new(ES_HASH);
pub const FS_MID: DManifestId = DManifestId::new(FS_HASH);

// Definitions for filenode IDs 1111...ffff
pub const ONES_FNID: DFileNodeId = DFileNodeId::new(ONES_HASH);
pub const TWOS_FNID: DFileNodeId = DFileNodeId::new(TWOS_HASH);
pub const THREES_FNID: DFileNodeId = DFileNodeId::new(THREES_HASH);
pub const FOURS_FNID: DFileNodeId = DFileNodeId::new(FOURS_HASH);
pub const FIVES_FNID: DFileNodeId = DFileNodeId::new(FIVES_HASH);
pub const SIXES_FNID: DFileNodeId = DFileNodeId::new(SIXES_HASH);
pub const SEVENS_FNID: DFileNodeId = DFileNodeId::new(SEVENS_HASH);
pub const EIGHTS_FNID: DFileNodeId = DFileNodeId::new(EIGHTS_HASH);
pub const NINES_FNID: DFileNodeId = DFileNodeId::new(NINES_HASH);
pub const AS_FNID: DFileNodeId = DFileNodeId::new(AS_HASH);
pub const BS_FNID: DFileNodeId = DFileNodeId::new(BS_HASH);
pub const CS_FNID: DFileNodeId = DFileNodeId::new(CS_HASH);
pub const DS_FNID: DFileNodeId = DFileNodeId::new(DS_HASH);
pub const ES_FNID: DFileNodeId = DFileNodeId::new(ES_HASH);
pub const FS_FNID: DFileNodeId = DFileNodeId::new(FS_HASH);
