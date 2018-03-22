// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mononoke_types::ContentId;

use hash;

// Definitions for hashes 1111...ffff.
pub const ONES_BLOB: ContentId = ContentId::new(hash::ONES);
pub const TWOS_BLOB: ContentId = ContentId::new(hash::TWOS);
pub const THREES_BLOB: ContentId = ContentId::new(hash::THREES);
pub const FOURS_BLOB: ContentId = ContentId::new(hash::FOURS);
pub const FIVES_BLOB: ContentId = ContentId::new(hash::FIVES);
pub const SIXES_BLOB: ContentId = ContentId::new(hash::SIXES);
pub const SEVENS_BLOB: ContentId = ContentId::new(hash::SEVENS);
pub const EIGHTS_BLOB: ContentId = ContentId::new(hash::EIGHTS);
pub const NINES_BLOB: ContentId = ContentId::new(hash::NINES);
pub const AS_BLOB: ContentId = ContentId::new(hash::AS);
pub const BS_BLOB: ContentId = ContentId::new(hash::BS);
pub const CS_BLOB: ContentId = ContentId::new(hash::CS);
pub const DS_BLOB: ContentId = ContentId::new(hash::DS);
pub const ES_BLOB: ContentId = ContentId::new(hash::ES);
pub const FS_BLOB: ContentId = ContentId::new(hash::FS);
