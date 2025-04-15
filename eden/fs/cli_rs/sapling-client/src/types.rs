/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[derive(Clone, Debug, PartialEq)]
pub enum SaplingStatus {
    Modified,
    Added,
    Removed,
    Clean,
    Missing,
    NotTracked,
    Ignored,
    Copied,
}
