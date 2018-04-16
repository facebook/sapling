// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types_mocks::hash;

use nodehash::HgNodeHash;

// Definitions for hashes 1111...ffff.
pub const ONES_HASH: HgNodeHash = HgNodeHash(hash::ONES);
pub const TWOS_HASH: HgNodeHash = HgNodeHash(hash::TWOS);
pub const THREES_HASH: HgNodeHash = HgNodeHash(hash::THREES);
pub const FOURS_HASH: HgNodeHash = HgNodeHash(hash::FOURS);
pub const FIVES_HASH: HgNodeHash = HgNodeHash(hash::FIVES);
pub const SIXES_HASH: HgNodeHash = HgNodeHash(hash::SIXES);
pub const SEVENS_HASH: HgNodeHash = HgNodeHash(hash::SEVENS);
pub const EIGHTS_HASH: HgNodeHash = HgNodeHash(hash::EIGHTS);
pub const NINES_HASH: HgNodeHash = HgNodeHash(hash::NINES);
pub const AS_HASH: HgNodeHash = HgNodeHash(hash::AS);
pub const BS_HASH: HgNodeHash = HgNodeHash(hash::BS);
pub const CS_HASH: HgNodeHash = HgNodeHash(hash::CS);
pub const DS_HASH: HgNodeHash = HgNodeHash(hash::DS);
pub const ES_HASH: HgNodeHash = HgNodeHash(hash::ES);
pub const FS_HASH: HgNodeHash = HgNodeHash(hash::FS);
