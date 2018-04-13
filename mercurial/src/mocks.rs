// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types_mocks::hash;

use nodehash::NodeHash;

// Definitions for hashes 1111...ffff.
pub const ONES_HASH: NodeHash = NodeHash(hash::ONES);
pub const TWOS_HASH: NodeHash = NodeHash(hash::TWOS);
pub const THREES_HASH: NodeHash = NodeHash(hash::THREES);
pub const FOURS_HASH: NodeHash = NodeHash(hash::FOURS);
pub const FIVES_HASH: NodeHash = NodeHash(hash::FIVES);
pub const SIXES_HASH: NodeHash = NodeHash(hash::SIXES);
pub const SEVENS_HASH: NodeHash = NodeHash(hash::SEVENS);
pub const EIGHTS_HASH: NodeHash = NodeHash(hash::EIGHTS);
pub const NINES_HASH: NodeHash = NodeHash(hash::NINES);
pub const AS_HASH: NodeHash = NodeHash(hash::AS);
pub const BS_HASH: NodeHash = NodeHash(hash::BS);
pub const CS_HASH: NodeHash = NodeHash(hash::CS);
pub const DS_HASH: NodeHash = NodeHash(hash::DS);
pub const ES_HASH: NodeHash = NodeHash(hash::ES);
pub const FS_HASH: NodeHash = NodeHash(hash::FS);
