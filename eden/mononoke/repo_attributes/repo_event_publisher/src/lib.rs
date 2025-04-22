/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo Event Publisher.
//!
//! Responsible for publishing repo events (e.g. bookmark updates, tag updates, etc.)
//! and allowing interested parties to subscribe to them.

#![feature(trait_alias)]

#[cfg(fbcode_build)]
mod facebook;
mod from_scuba_json;
#[cfg(not(fbcode_build))]
mod oss;
mod repo_name_provider;

use anyhow::Result;
#[cfg(fbcode_build)]
pub use facebook::scribe_listener::ScribeListener;
#[cfg(fbcode_build)]
pub use facebook::scribe_repo_event_publisher::ScribeRepoEventPublisher;
#[cfg(not(fbcode_build))]
pub use oss::UnsupportedRepoEventPublisher;
use repo_update_logger::PlainBookmarkInfo;
use tokio::sync::broadcast;

/// The name of the repo.
pub(crate) type RepoName = String;

/// The core Repo Event Publisher facet.
#[facet::facet]
#[allow(dead_code)]
pub trait RepoEventPublisher {
    /// Subscribe to bookmark create/update/delete notifications for the repo.
    fn subscribe_for_bookmark_updates(
        &self,
        repo_name: &RepoName,
    ) -> Result<broadcast::Receiver<PlainBookmarkInfo>>;
}
