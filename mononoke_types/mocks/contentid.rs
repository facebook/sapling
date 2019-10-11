/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::hash;
use mononoke_types::ContentId;

// Definitions for hashes 1111...ffff.
pub const ONES_CTID: ContentId = ContentId::new(hash::ONES);
pub const TWOS_CTID: ContentId = ContentId::new(hash::TWOS);
pub const THREES_CTID: ContentId = ContentId::new(hash::THREES);
pub const FOURS_CTID: ContentId = ContentId::new(hash::FOURS);
pub const FIVES_CTID: ContentId = ContentId::new(hash::FIVES);
pub const SIXES_CTID: ContentId = ContentId::new(hash::SIXES);
pub const SEVENS_CTID: ContentId = ContentId::new(hash::SEVENS);
pub const EIGHTS_CTID: ContentId = ContentId::new(hash::EIGHTS);
pub const NINES_CTID: ContentId = ContentId::new(hash::NINES);
pub const AS_CTID: ContentId = ContentId::new(hash::AS);
pub const BS_CTID: ContentId = ContentId::new(hash::BS);
pub const CS_CTID: ContentId = ContentId::new(hash::CS);
pub const DS_CTID: ContentId = ContentId::new(hash::DS);
pub const ES_CTID: ContentId = ContentId::new(hash::ES);
pub const FS_CTID: ContentId = ContentId::new(hash::FS);
