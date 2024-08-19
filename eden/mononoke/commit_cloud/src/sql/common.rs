/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UpdateWorkspaceNameArgs {
    pub new_workspace: String,
}
