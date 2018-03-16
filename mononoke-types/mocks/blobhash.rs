// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mononoke_types::BlobHash;

use hash;

// Definitions for hashes 1111...ffff.
pub const ONES_BLOB: BlobHash = BlobHash::new_mock(hash::ONES);
pub const TWOS_BLOB: BlobHash = BlobHash::new_mock(hash::TWOS);
pub const THREES_BLOB: BlobHash = BlobHash::new_mock(hash::THREES);
pub const FOURS_BLOB: BlobHash = BlobHash::new_mock(hash::FOURS);
pub const FIVES_BLOB: BlobHash = BlobHash::new_mock(hash::FIVES);
pub const SIXES_BLOB: BlobHash = BlobHash::new_mock(hash::SIXES);
pub const SEVENS_BLOB: BlobHash = BlobHash::new_mock(hash::SEVENS);
pub const EIGHTS_BLOB: BlobHash = BlobHash::new_mock(hash::EIGHTS);
pub const NINES_BLOB: BlobHash = BlobHash::new_mock(hash::NINES);
pub const AS_BLOB: BlobHash = BlobHash::new_mock(hash::AS);
pub const BS_BLOB: BlobHash = BlobHash::new_mock(hash::BS);
pub const CS_BLOB: BlobHash = BlobHash::new_mock(hash::CS);
pub const DS_BLOB: BlobHash = BlobHash::new_mock(hash::DS);
pub const ES_BLOB: BlobHash = BlobHash::new_mock(hash::ES);
pub const FS_BLOB: BlobHash = BlobHash::new_mock(hash::FS);
