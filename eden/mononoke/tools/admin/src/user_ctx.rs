/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use context::CoreContext;
use context::SessionContainer;
use metadata::Metadata;
use mononoke_app::MononokeApp;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;

/// Build a basic `CoreContext` for admin commands that attributes actions to
/// the invoking Unix user (from `$USER`). Used where downstream code reads
/// `ctx.metadata().unix_name()` — e.g. the `created_by` column on
/// long-running async request rows — so the operator who ran the command is
/// recorded instead of nothing.
///
/// Falls back to `app.new_basic_context()` when `$USER` is unset or empty.
pub(crate) fn new_basic_context_with_unixname(app: &MononokeApp) -> CoreContext {
    let ctx = app.new_basic_context();
    let unixname = match std::env::var("USER") {
        Ok(u) if !u.is_empty() => u,
        _ => return ctx,
    };
    let mut identities = MononokeIdentitySet::new();
    identities.insert(MononokeIdentity::from_legacy_type_data("USER", &unixname));
    let metadata = Arc::new(Metadata::default().set_identities(identities));
    let session = SessionContainer::builder(ctx.fb).metadata(metadata).build();
    session.new_context(ctx.scuba().clone())
}
