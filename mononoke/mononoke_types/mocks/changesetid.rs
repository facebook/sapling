/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use mononoke_types::ChangesetId;

use crate::hash;

// Definitions for hashes 1111...ffff.
pub const ONES_CSID: ChangesetId = ChangesetId::new(hash::ONES);
pub const TWOS_CSID: ChangesetId = ChangesetId::new(hash::TWOS);
pub const THREES_CSID: ChangesetId = ChangesetId::new(hash::THREES);
pub const FOURS_CSID: ChangesetId = ChangesetId::new(hash::FOURS);
pub const FIVES_CSID: ChangesetId = ChangesetId::new(hash::FIVES);
pub const SIXES_CSID: ChangesetId = ChangesetId::new(hash::SIXES);
pub const SEVENS_CSID: ChangesetId = ChangesetId::new(hash::SEVENS);
pub const EIGHTS_CSID: ChangesetId = ChangesetId::new(hash::EIGHTS);
pub const NINES_CSID: ChangesetId = ChangesetId::new(hash::NINES);
pub const AS_CSID: ChangesetId = ChangesetId::new(hash::AS);
pub const BS_CSID: ChangesetId = ChangesetId::new(hash::BS);
pub const CS_CSID: ChangesetId = ChangesetId::new(hash::CS);
pub const DS_CSID: ChangesetId = ChangesetId::new(hash::DS);
pub const ES_CSID: ChangesetId = ChangesetId::new(hash::ES);
pub const FS_CSID: ChangesetId = ChangesetId::new(hash::FS);
