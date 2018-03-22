// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mononoke_types::UnodeId;

use hash;

// Definitions for hashes 1111...ffff.
pub const ONES_UNODE: UnodeId = UnodeId::new(hash::ONES);
pub const TWOS_UNODE: UnodeId = UnodeId::new(hash::TWOS);
pub const THREES_UNODE: UnodeId = UnodeId::new(hash::THREES);
pub const FOURS_UNODE: UnodeId = UnodeId::new(hash::FOURS);
pub const FIVES_UNODE: UnodeId = UnodeId::new(hash::FIVES);
pub const SIXES_UNODE: UnodeId = UnodeId::new(hash::SIXES);
pub const SEVENS_UNODE: UnodeId = UnodeId::new(hash::SEVENS);
pub const EIGHTS_UNODE: UnodeId = UnodeId::new(hash::EIGHTS);
pub const NINES_UNODE: UnodeId = UnodeId::new(hash::NINES);
pub const AS_UNODE: UnodeId = UnodeId::new(hash::AS);
pub const BS_UNODE: UnodeId = UnodeId::new(hash::BS);
pub const CS_UNODE: UnodeId = UnodeId::new(hash::CS);
pub const DS_UNODE: UnodeId = UnodeId::new(hash::DS);
pub const ES_UNODE: UnodeId = UnodeId::new(hash::ES);
pub const FS_UNODE: UnodeId = UnodeId::new(hash::FS);
