/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A subset of types that are related to wire protocols and used by the dag crate.

pub mod clone;
pub mod id;
pub mod location;
pub mod segment;

pub use clone::CloneData;
pub use id::Bytes;
pub use id::Group;
pub use id::Id;
pub use id::IdIter;
pub use id::VertexName;
pub use location::Location;
pub use segment::FlatSegment;
pub use segment::PreparedFlatSegments;
