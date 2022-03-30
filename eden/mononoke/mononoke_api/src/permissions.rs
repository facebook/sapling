/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Describes the permissions model that is being used to determine if a write is
/// permitted or not.
pub enum WritePermissionsModel {
    /// Writes are checked against the actions that a particular service may perform.
    ServiceIdentity(String),

    /// Any valid write is permitted.
    AllowAnyWrite,
}
