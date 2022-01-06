/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod checker;
#[cfg(fbcode_build)]
mod facebook;
mod identity;
mod membership;
#[cfg(not(fbcode_build))]
mod oss;

pub use checker::{
    ArcPermissionChecker, BoxPermissionChecker, PermissionChecker, PermissionCheckerBuilder,
};
pub use identity::{MononokeIdentity, MononokeIdentitySet, MononokeIdentitySetExt};
pub use membership::{
    ArcMembershipChecker, BoxMembershipChecker, MembershipChecker, MembershipCheckerBuilder,
};
