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

pub use checker::ArcPermissionChecker;
pub use checker::BoxPermissionChecker;
pub use checker::PermissionChecker;
pub use checker::PermissionCheckerBuilder;
pub use identity::MononokeIdentity;
pub use identity::MononokeIdentitySet;
pub use identity::MononokeIdentitySetExt;
pub use membership::ArcMembershipChecker;
pub use membership::BoxMembershipChecker;
pub use membership::MembershipChecker;
pub use membership::MembershipCheckerBuilder;
