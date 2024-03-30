/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use sha1::Sha1;

#[allow(unused)]
pub(crate) struct WorkspaceSnapshot {
    node: Sha1,
}
