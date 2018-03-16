// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mononoke_types::UnodeHash;

use hash;

// Definitions for hashes 1111...ffff.
pub const ONES_UNODE: UnodeHash = UnodeHash::new_mock(hash::ONES);
pub const TWOS_UNODE: UnodeHash = UnodeHash::new_mock(hash::TWOS);
pub const THREES_UNODE: UnodeHash = UnodeHash::new_mock(hash::THREES);
pub const FOURS_UNODE: UnodeHash = UnodeHash::new_mock(hash::FOURS);
pub const FIVES_UNODE: UnodeHash = UnodeHash::new_mock(hash::FIVES);
pub const SIXES_UNODE: UnodeHash = UnodeHash::new_mock(hash::SIXES);
pub const SEVENS_UNODE: UnodeHash = UnodeHash::new_mock(hash::SEVENS);
pub const EIGHTS_UNODE: UnodeHash = UnodeHash::new_mock(hash::EIGHTS);
pub const NINES_UNODE: UnodeHash = UnodeHash::new_mock(hash::NINES);
pub const AS_UNODE: UnodeHash = UnodeHash::new_mock(hash::AS);
pub const BS_UNODE: UnodeHash = UnodeHash::new_mock(hash::BS);
pub const CS_UNODE: UnodeHash = UnodeHash::new_mock(hash::CS);
pub const DS_UNODE: UnodeHash = UnodeHash::new_mock(hash::DS);
pub const ES_UNODE: UnodeHash = UnodeHash::new_mock(hash::ES);
pub const FS_UNODE: UnodeHash = UnodeHash::new_mock(hash::FS);
