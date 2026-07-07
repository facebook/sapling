/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;

pub struct ArtilleryTraceGuard;

pub fn continue_trace_from_context(
    _fb: FacebookInit,
    _context_bytes: &[u8],
) -> Option<ArtilleryTraceGuard> {
    None
}
