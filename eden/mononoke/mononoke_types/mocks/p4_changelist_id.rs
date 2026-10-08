/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::P4ChangelistId;

// Perforce changelist numbering starts at 1 -- there is no changelist 0.
pub const P4_CHANGELIST_ONE: P4ChangelistId = P4ChangelistId::new(1);
pub const P4_CHANGELIST_TWO: P4ChangelistId = P4ChangelistId::new(2);
pub const P4_CHANGELIST_THREE: P4ChangelistId = P4ChangelistId::new(3);
pub const P4_CHANGELIST_FOUR: P4ChangelistId = P4ChangelistId::new(4);
