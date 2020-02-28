/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
#[repr(u32)]
pub enum HgPhase {
    Public = 0,
    Draft,
}
