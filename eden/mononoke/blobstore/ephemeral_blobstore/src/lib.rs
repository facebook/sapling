/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Blobstore
//!
//! Unlike other blobstores, this blobstore organises blobs into ephemeral
//! "bubbles".  Blobs placed within a bubble are deleted when the bubble
//! expires.  A bubble's lifespan can be extended at any time before it
//! expires.

mod bubble;
mod builder;
mod changesets;
mod error;
mod file;
mod handle;
mod store;
mod view;

pub use crate::bubble::Bubble;
pub use crate::bubble::BubbleId;
pub use crate::bubble::StorageLocation;
pub use crate::builder::RepoEphemeralStoreBuilder;
pub use crate::changesets::EphemeralChangesets;
pub use crate::error::EphemeralBlobstoreError;
pub use crate::handle::EphemeralHandle;
pub use crate::store::ArcRepoEphemeralStore;
pub use crate::store::RepoEphemeralStore;
pub use crate::store::RepoEphemeralStoreArc;
pub use crate::store::RepoEphemeralStoreRef;
pub use crate::view::EphemeralRepoView;
