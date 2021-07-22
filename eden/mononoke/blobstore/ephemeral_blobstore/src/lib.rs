/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
mod error;
mod handle;
mod repo;
mod store;

pub use crate::bubble::{Bubble, BubbleId};
pub use crate::builder::EphemeralBlobstoreBuilder;
pub use crate::error::EphemeralBlobstoreError;
pub use crate::handle::EphemeralHandle;
pub use crate::repo::{ArcRepoEphemeralBlobstore, RepoEphemeralBlobstore};
pub use crate::store::EphemeralBlobstore;
