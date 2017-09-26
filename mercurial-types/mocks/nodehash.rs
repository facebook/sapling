// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// NULL_HASH is exported for convenience.
pub use mercurial_types::NULL_HASH;
use mercurial_types::NodeHash;

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
