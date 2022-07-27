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
mod internal;
mod membership;
#[cfg(not(fbcode_build))]
mod oss;
mod provider;

pub use checker::ArcPermissionChecker;
pub use checker::BoxPermissionChecker;
pub use checker::PermissionChecker;
pub use checker::PermissionCheckerBuilder;
pub use identity::pretty_print;
pub use identity::MononokeIdentity;
pub use identity::MononokeIdentitySet;
pub use identity::MononokeIdentitySetExt;
pub use internal::InternalAclProvider;
pub use membership::AlwaysMember;
pub use membership::ArcMembershipChecker;
pub use membership::BoxMembershipChecker;
pub use membership::MemberAllowlist;
pub use membership::MembershipChecker;
pub use membership::NeverMember;
pub use provider::AclProvider;

#[cfg(fbcode_build)]
pub type DefaultAclProvider = facebook::HipsterAclProvider;

#[cfg(not(fbcode_build))]
pub type DefaultAclProvider = oss::DummyAclProvider;
